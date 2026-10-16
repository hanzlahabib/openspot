//! IE parsing throughput on realistic beacons (Q-04).

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use openspot_ie::{Band, Context, parse};

fn el(id: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![id, u8::try_from(body.len()).expect("short body")];
    v.extend_from_slice(body);
    v
}

fn ext(ext_id: u8, body: &[u8]) -> Vec<u8> {
    el(255, &[&[ext_id][..], body].concat())
}

/// A Wi-Fi 6E style beacon tail: rates, HT/VHT/HE/EHT capability and operation elements,
/// RSN with SAE, plus a vendor element, in the order APs send them.
fn rich_beacon() -> Vec<u8> {
    let rsn = el(48, &[1, 0, 0x00, 0x0F, 0xAC, 4, 1, 0, 0x00, 0x0F, 0xAC, 4, 1, 0, 0x00, 0x0F, 0xAC, 8, 0, 0]);
    [
        el(1, &[0x8C, 0x12, 0x98, 0x24, 0xB0, 0x48, 0x60, 0x6C]),
        el(3, &[36]),
        el(45, &[0; 26]),
        el(61, &[36, 0b101, 0, 0, 0, 0]),
        el(191, &[0; 12]),
        el(192, &[1, 42, 0, 0, 0]),
        ext(35, &[0; 20]),
        ext(36, &[0, 0, 0x02, 0, 0, 0, 5, 0b10, 7, 0, 0]),
        ext(108, &[0; 20]),
        ext(106, &[1, 0, 0, 0, 0, 4, 31, 15]),
        rsn,
        el(221, &[0x00, 0x50, 0xF2, 0x02, 0, 0, 0, 0, 0, 0, 0, 0]),
    ]
    .concat()
}

fn bench_parse(c: &mut Criterion) {
    let beacon = rich_beacon();
    let ctx = Context { band: Some(Band::Ghz5), privacy: true };
    let mut group = c.benchmark_group("ie");
    group.throughput(Throughput::Bytes(beacon.len() as u64));
    group.bench_function("parse_wifi7_beacon", |b| b.iter(|| parse(black_box(&beacon), ctx)));
    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
