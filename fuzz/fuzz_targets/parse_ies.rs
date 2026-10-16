#![no_main]

use libfuzzer_sys::fuzz_target;
use openspot_ie::{Band, Context, Elements, parse};

// First byte selects the frame context, the rest is the raw IE run. The parser must
// neither panic nor loop on any input.
fuzz_target!(|data: &[u8]| {
    let Some((&flags, ies)) = data.split_first() else { return };
    let band = match flags & 0b11 {
        0 => None,
        1 => Some(Band::Ghz2_4),
        2 => Some(Band::Ghz5),
        _ => Some(Band::Ghz6),
    };
    let parsed = parse(ies, Context { band, privacy: flags & 0b100 != 0 });
    assert!(!parsed.security.is_empty());
    // Iteration must consume strictly forward: the count is bounded by input length / 2.
    assert!(Elements::new(ies).count() <= ies.len() / 2);
});
