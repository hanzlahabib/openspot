//! HTTP download/upload speed tests (roadmap A-02).
//!
//! Download: GET a URL and count the body bytes, timing from the first byte of the response
//! (headers received) to the end so DNS, TCP and TLS setup do not depress the figure.
//! Upload: POST a generated payload and time from the start of the send until the server's
//! response arrives, which proves the server consumed the data.
//!
//! UNVERIFIED against real internet servers; the tests use a local `TcpListener` server.

use std::io::{self, Read};
use std::time::{Duration, Instant};

use ureq::Agent;

use crate::{CancelToken, SpeedError, mbps};

/// Read/write granularity. 64 KiB keeps cancellation latency far below a millisecond at
/// typical Wi-Fi speeds without making the per-chunk bookkeeping show up in the numbers.
const CHUNK: usize = 64 * 1024;

/// Retries of a request interrupted by a signal before any data moved.
const MAX_EINTR_RETRIES: u32 = 3;

/// Default test duration cap; a stuck transfer must not hang a survey point forever.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Settings shared by both directions.
#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub url: String,
    /// Download: stop reading after this many bytes (so an unbounded or huge file still gives
    /// a bounded test); `None` reads to the end. Upload: exact number of bytes posted.
    pub payload_bytes: u64,
    /// Hard limit for the whole request, including connection setup.
    pub timeout: Duration,
}

impl HttpConfig {
    pub fn new(url: impl Into<String>, payload_bytes: u64) -> Self {
        Self { url: url.into(), payload_bytes, timeout: DEFAULT_TIMEOUT }
    }
}

/// One finished transfer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HttpMeasurement {
    pub bytes: u64,
    pub elapsed: Duration,
    pub mbps: f64,
}

fn agent(timeout: Duration) -> Agent {
    // Non-2xx must surface as `Status`, so turn off ureq's own status-as-error handling.
    Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).build().into()
}

fn map_err(e: ureq::Error, timeout: Duration) -> SpeedError {
    match e {
        ureq::Error::Timeout(_) => SpeedError::Timeout(timeout),
        other => SpeedError::Http(other.to_string()),
    }
}

fn check_config(cfg: &HttpConfig) -> Result<(), SpeedError> {
    if cfg.url.trim().is_empty() {
        return Err(SpeedError::Config("URL is empty".into()));
    }
    if cfg.payload_bytes == 0 {
        return Err(SpeedError::Config("payload size must be greater than zero".into()));
    }
    Ok(())
}

fn finish(bytes: u64, elapsed: Duration) -> Result<HttpMeasurement, SpeedError> {
    let mbps = mbps(bytes, elapsed).filter(|_| bytes > 0).ok_or(SpeedError::TooShort { bytes, elapsed })?;
    Ok(HttpMeasurement { bytes, elapsed, mbps })
}

/// Downloads `cfg.url` (up to `cfg.payload_bytes`) and returns the achieved throughput.
pub fn download(cfg: &HttpConfig, cancel: &CancelToken) -> Result<HttpMeasurement, SpeedError> {
    check_config(cfg)?;
    if cancel.is_cancelled() {
        return Err(SpeedError::Cancelled);
    }
    let agent = agent(cfg.timeout);
    // A SIGCHLD from an unrelated child process (e.g. iperf3 finishing in another thread) can
    // interrupt the blocking connect/read with EINTR, which ureq surfaces instead of retrying.
    // Nothing has been transferred yet, so retrying the request is safe.
    let mut attempt = 0;
    let mut response = loop {
        match agent.get(&cfg.url).call() {
            Err(ureq::Error::Io(e)) if e.kind() == io::ErrorKind::Interrupted && attempt < MAX_EINTR_RETRIES => attempt += 1,
            other => break other.map_err(|e| map_err(e, cfg.timeout))?,
        }
    };
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(SpeedError::Status(status));
    }
    let mut reader = response.body_mut().with_config().limit(cfg.payload_bytes).reader();
    let mut buf = vec![0u8; CHUNK];
    let (mut total, start) = (0u64, Instant::now());
    loop {
        if cancel.is_cancelled() {
            return Err(SpeedError::Cancelled);
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => total += n as u64,
            // The limit reader reports "exceeded" once the cap is hit; that is a normal end.
            Err(e) if total >= cfg.payload_bytes && is_limit_error(&e) => break,
            Err(e) => return Err(io_err(e, cfg.timeout)),
        }
        if total >= cfg.payload_bytes {
            break;
        }
    }
    finish(total, start.elapsed())
}

fn is_limit_error(e: &io::Error) -> bool {
    e.to_string().contains("limit")
}

fn io_err(e: io::Error, timeout: Duration) -> SpeedError {
    if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) {
        return SpeedError::Timeout(timeout);
    }
    // ureq wraps its own errors in io::Error.
    match e.downcast::<ureq::Error>() {
        Ok(inner) => map_err(inner, timeout),
        Err(e) => SpeedError::Http(e.to_string()),
    }
}

/// Endless-looking zero payload of fixed length that stops when cancelled.
struct Payload<'a> {
    remaining: u64,
    cancel: &'a CancelToken,
}

impl Read for Payload<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = buf.len().min(CHUNK).min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        buf[..n].fill(0);
        self.remaining -= n as u64;
        Ok(n)
    }
}

/// POSTs `cfg.payload_bytes` of zeros to `cfg.url` and returns the achieved throughput.
pub fn upload(cfg: &HttpConfig, cancel: &CancelToken) -> Result<HttpMeasurement, SpeedError> {
    check_config(cfg)?;
    if cancel.is_cancelled() {
        return Err(SpeedError::Cancelled);
    }
    let mut payload = Payload { remaining: cfg.payload_bytes, cancel };
    let start = Instant::now();
    let result = agent(cfg.timeout)
        .post(&cfg.url)
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", cfg.payload_bytes.to_string())
        .send(ureq::SendBody::from_reader(&mut payload));
    if cancel.is_cancelled() {
        return Err(SpeedError::Cancelled);
    }
    let response = result.map_err(|e| map_err(e, cfg.timeout))?;
    let elapsed = start.elapsed();
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(SpeedError::Status(status));
    }
    finish(cfg.payload_bytes, elapsed)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    use super::*;

    /// How a test server should answer.
    #[derive(Clone, Copy)]
    enum Mode {
        /// Serve `n` body bytes to GET; consume Content-Length bytes of a POST and answer 200.
        Ok(u64),
        Status(u16),
        /// Accept the connection and never answer.
        Silent,
    }

    fn serve(mode: Mode) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                thread::spawn(move || handle(stream, mode));
            }
        });
        format!("http://{addr}/file")
    }

    fn handle(stream: TcpStream, mode: Mode) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let is_post = line.starts_with("POST");
        let mut content_length = 0u64;
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = v.trim().parse().unwrap();
            }
        }
        let mut out = stream;
        match mode {
            Mode::Silent => thread::sleep(Duration::from_secs(5)),
            Mode::Status(code) => {
                // Drain the body first: closing with unread data makes Windows send a reset,
                // which would hide the status line from the client.
                if is_post {
                    let _ = io::copy(&mut reader.take(content_length), &mut io::sink());
                }
                let _ = write!(out, "HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
            Mode::Ok(n) if is_post => {
                io::copy(&mut reader.take(content_length), &mut io::sink()).unwrap();
                let _ = write!(out, "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = n;
            }
            Mode::Ok(n) => {
                let _ = write!(out, "HTTP/1.1 200 OK\r\nContent-Length: {n}\r\nConnection: close\r\n\r\n");
                let chunk = vec![7u8; CHUNK];
                let mut left = n;
                while left > 0 {
                    let k = left.min(CHUNK as u64) as usize;
                    if out.write_all(&chunk[..k]).is_err() {
                        break;
                    }
                    left -= k as u64;
                }
            }
        }
    }

    #[test]
    fn download_counts_every_byte_and_derives_mbps_from_them() {
        let url = serve(Mode::Ok(3_000_000));
        let m = download(&HttpConfig::new(url, 10_000_000), &CancelToken::new()).unwrap();
        assert_eq!(m.bytes, 3_000_000);
        let expected = 3_000_000.0 * 8.0 / m.elapsed.as_secs_f64() / 1e6;
        assert!((m.mbps - expected).abs() < 1e-6 * expected.max(1.0), "{} vs {expected}", m.mbps);
        assert!(m.mbps > 0.0);
    }

    #[test]
    fn download_stops_at_the_payload_cap() {
        let url = serve(Mode::Ok(5_000_000));
        let m = download(&HttpConfig::new(url, 1_000_000), &CancelToken::new()).unwrap();
        assert_eq!(m.bytes, 1_000_000);
    }

    #[test]
    fn upload_posts_exactly_the_payload() {
        let url = serve(Mode::Ok(0));
        let m = upload(&HttpConfig::new(url, 2_500_000), &CancelToken::new()).unwrap();
        assert_eq!(m.bytes, 2_500_000);
        assert!(m.mbps > 0.0);
    }

    #[test]
    fn non_2xx_is_a_status_error() {
        let url = serve(Mode::Status(404));
        assert!(matches!(download(&HttpConfig::new(url.clone(), 10), &CancelToken::new()), Err(SpeedError::Status(404))));
        assert!(matches!(upload(&HttpConfig::new(url, 10), &CancelToken::new()), Err(SpeedError::Status(404))));
    }

    #[test]
    fn silent_server_times_out() {
        let url = serve(Mode::Silent);
        let mut cfg = HttpConfig::new(url, 10);
        cfg.timeout = Duration::from_millis(300);
        let started = Instant::now();
        let r = download(&cfg, &CancelToken::new());
        assert!(matches!(r, Err(SpeedError::Timeout(_))), "{r:?}");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn cancelled_before_start_aborts_both_directions() {
        let url = serve(Mode::Ok(1_000_000));
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(matches!(download(&HttpConfig::new(url.clone(), 1_000_000), &cancel), Err(SpeedError::Cancelled)));
        assert!(matches!(upload(&HttpConfig::new(url, 1_000_000), &cancel), Err(SpeedError::Cancelled)));
    }

    #[test]
    fn bad_config_is_rejected() {
        let c = CancelToken::new();
        assert!(matches!(download(&HttpConfig::new("", 10), &c), Err(SpeedError::Config(_))));
        assert!(matches!(upload(&HttpConfig::new("http://x", 0), &c), Err(SpeedError::Config(_))));
    }
}
