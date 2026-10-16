//! Active-scan throughput tests: HTTP download/upload ([`http`]) and the system `iperf3`
//! ([`iperf`]), combined by [`suite`] into one [`openspot_model::SpeedResult`] per network.
//!
//! No test here has been run against a real network or a real `iperf3` server by the
//! authors of this crate; see the UNVERIFIED notes on the individual modules.

#![forbid(unsafe_code)]

pub mod http;
pub mod iperf;
pub mod suite;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use thiserror::Error;

/// Bits in a megabit as used for throughput (SI, 10^6), matching iperf3's `bits_per_second`.
pub const BITS_PER_MEGABIT: f64 = 1_000_000.0;

/// Failures of one throughput test.
#[derive(Debug, Error)]
pub enum SpeedError {
    #[error("cancelled")]
    Cancelled,
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("server answered HTTP {0}")]
    Status(u16),
    #[error("transfer too short to measure ({bytes} bytes in {elapsed:?})")]
    TooShort { bytes: u64, elapsed: Duration },
    #[error("could not start iperf3 (is it installed and on PATH?): {0}")]
    IperfSpawn(String),
    #[error("iperf3 reported an error: {0}")]
    Iperf(String),
    #[error("could not parse iperf3 output: {0}")]
    IperfOutput(String),
    #[error("invalid setting: {0}")]
    Config(String),
}

/// Cooperative cancellation shared between the UI and a running test. Cloning shares state.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Throughput in megabits per second for `bytes` moved in `elapsed`; `None` when `elapsed`
/// is zero (cannot divide) so callers never publish an infinite speed.
pub fn mbps(bytes: u64, elapsed: Duration) -> Option<f64> {
    let secs = elapsed.as_secs_f64();
    (secs > 0.0).then(|| bytes as f64 * 8.0 / secs / BITS_PER_MEGABIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mbps_math() {
        // 1_250_000 bytes = 10 Mbit; in 2 s that is 5 Mbps.
        assert_eq!(mbps(1_250_000, Duration::from_secs(2)), Some(5.0));
        assert_eq!(mbps(0, Duration::from_secs(1)), Some(0.0));
        assert_eq!(mbps(10, Duration::ZERO), None);
    }

    #[test]
    fn cancel_is_shared_between_clones() {
        let a = CancelToken::new();
        let b = a.clone();
        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
    }
}
