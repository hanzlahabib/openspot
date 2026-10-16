//! Vendor lookup from the IEEE registries: 24-bit MA-L, 28-bit MA-M and 36-bit MA-S
//! assignments, matched longest prefix first.
//!
//! The IEEE sells the same 48-bit address space in three block sizes. A small company gets a
//! 28- or 36-bit block carved out of an MA-L whose own entry only names the registration
//! authority, so a lookup must try the longer prefixes before falling back to the 24-bit one.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::model::MacAddress;

/// `OUI<TAB>organization` per line, see `data/README.md` for provenance and how to refresh.
const MA_L_TSV: &str = include_str!("../data/oui.tsv");
/// Same format with 7-hex-digit (28-bit) prefixes.
const MA_M_TSV: &str = include_str!("../data/oui28.tsv");
/// Same format with 9-hex-digit (36-bit) prefixes.
const MA_S_TSV: &str = include_str!("../data/oui36.tsv");

/// Bit 1 of the first octet: set for locally administered (e.g. randomized/private) addresses.
const LOCALLY_ADMINISTERED: u8 = 0x02;

/// Address bits in a MAC address.
const ADDRESS_BITS: u32 = 48;
/// Prefix lengths of the three registries.
const MA_L_BITS: u32 = 24;
const MA_M_BITS: u32 = 28;
const MA_S_BITS: u32 = 36;

type Table = HashMap<u64, &'static str>;

/// Parses `prefix<TAB>name` lines. An empty name is kept: it marks a privately registered
/// block, which must hide the parent MA-L's name instead of falling through to it.
fn parse(tsv: &'static str) -> Table {
    tsv.lines()
        .filter_map(|line| {
            let (prefix, name) = line.split_once('\t')?;
            Some((u64::from_str_radix(prefix, 16).ok()?, name))
        })
        .collect()
}

/// The three registries together.
struct Registry {
    ma_l: Table,
    ma_m: Table,
    ma_s: Table,
}

impl Registry {
    fn new(ma_l: &'static str, ma_m: &'static str, ma_s: &'static str) -> Self {
        Self { ma_l: parse(ma_l), ma_m: parse(ma_m), ma_s: parse(ma_s) }
    }

    /// Organization of the longest registered prefix of `address`; `None` when nothing
    /// matches or the matching block is private.
    fn lookup(&self, address: u64) -> Option<&'static str> {
        [(&self.ma_s, MA_S_BITS), (&self.ma_m, MA_M_BITS), (&self.ma_l, MA_L_BITS)]
            .into_iter()
            .find_map(|(table, bits)| table.get(&(address >> (ADDRESS_BITS - bits))))
            .copied()
            .filter(|name| !name.is_empty())
    }
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry::new(MA_L_TSV, MA_M_TSV, MA_S_TSV))
}

impl MacAddress {
    /// Whether the address was assigned locally rather than by a manufacturer; such
    /// addresses (private Wi-Fi addresses, virtual interfaces) have no vendor.
    pub fn is_locally_administered(&self) -> bool {
        self.0[0] & LOCALLY_ADMINISTERED != 0
    }

    /// Registered manufacturer of the address, if any.
    pub fn vendor(&self) -> Option<&'static str> {
        if self.is_locally_administered() {
            return None;
        }
        let [a, b, c, d, e, f] = self.0;
        registry().lookup(u64::from_be_bytes([0, 0, a, b, c, d, e, f]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(bytes: [u8; 6]) -> u64 {
        let [a, b, c, d, e, f] = bytes;
        u64::from_be_bytes([0, 0, a, b, c, d, e, f])
    }

    /// Small synthetic registries; prefix `AA:BB:CC` is an MA-L with a nested MA-M and MA-S.
    fn synthetic() -> Registry {
        Registry::new(
            "AABBCC\tParent Authority\n112233\tPlain Co\n445566\tOther Authority\n",
            "AABBCCD\tMid Corp\n4455660\t\n",
            "AABBCCE00\tSmall Ltd\nAABBCCD01\tSmall In Mid Ltd\n",
        )
    }

    #[test]
    fn longest_prefix_wins() {
        let r = synthetic();
        assert_eq!(r.lookup(address([0xAA, 0xBB, 0xCC, 0xE0, 0x05, 0x06])), Some("Small Ltd"), "36-bit block");
        assert_eq!(r.lookup(address([0xAA, 0xBB, 0xCC, 0xD7, 0x05, 0x06])), Some("Mid Corp"), "28-bit block");
        assert_eq!(r.lookup(address([0xAA, 0xBB, 0xCC, 0xD0, 0x15, 0x06])), Some("Small In Mid Ltd"), "36 beats 28");
        assert_eq!(r.lookup(address([0xAA, 0xBB, 0xCC, 0x00, 0x00, 0x01])), Some("Parent Authority"), "falls back to 24-bit");
        assert_eq!(r.lookup(address([0x11, 0x22, 0x33, 0x44, 0x55, 0x66])), Some("Plain Co"));
    }

    #[test]
    fn a_neighbouring_block_does_not_match() {
        let r = synthetic();
        // 28-bit block AABBCCD ends at nibble D; E belongs to a different (here 36-bit) block.
        assert_eq!(r.lookup(address([0xAA, 0xBB, 0xCC, 0xE1, 0x00, 0x00])), Some("Parent Authority"));
        // The 36-bit block AABBCCE00 covers 0xE00xxxxx..0xE00FFFFF only: the nibble after E must be 0.
        assert_eq!(r.lookup(address([0xAA, 0xBB, 0xCC, 0xE0, 0x0F, 0xFF])), Some("Small Ltd"));
        assert_eq!(r.lookup(address([0xAA, 0xBB, 0xCC, 0xE0, 0x10, 0x00])), Some("Parent Authority"));
    }

    #[test]
    fn private_sub_blocks_hide_the_parent_name() {
        let r = synthetic();
        assert_eq!(r.lookup(address([0x44, 0x55, 0x66, 0x05, 0x00, 0x00])), None, "private MA-M block");
        assert_eq!(r.lookup(address([0x44, 0x55, 0x66, 0x15, 0x00, 0x00])), Some("Other Authority"), "other blocks still resolve");
    }

    #[test]
    fn unknown_prefixes_have_no_vendor() {
        assert_eq!(synthetic().lookup(address([0x99, 0x99, 0x99, 0, 0, 0])), None);
    }

    #[test]
    fn tables_are_loaded_and_large() {
        let r = registry();
        assert!(r.ma_l.len() > 30_000, "{}", r.ma_l.len());
        assert!(r.ma_m.len() > 5_000, "{}", r.ma_m.len());
        assert!(r.ma_s.len() > 5_000, "{}", r.ma_s.len());
    }

    #[test]
    fn known_registrations_resolve() {
        let vmware = MacAddress([0x00, 0x50, 0x56, 1, 2, 3]).vendor().unwrap();
        assert!(vmware.contains("VMware"), "{vmware}");
        let pi = MacAddress([0xB8, 0x27, 0xEB, 0, 0, 1]).vendor().unwrap();
        assert!(pi.contains("Raspberry"), "{pi}");
    }

    #[test]
    fn real_ma_m_block_resolves_below_its_parent_oui() {
        // 00:55:DA is an MA-L owned by the registration authority; 0055DA0 and 0055DA1 are
        // distinct 28-bit blocks inside it.
        assert_eq!(MacAddress([0x00, 0x55, 0xDA, 0x00, 0x12, 0x34]).vendor(), Some("Shinko Technos co.,ltd."));
        assert_eq!(MacAddress([0x00, 0x55, 0xDA, 0x1F, 0xFF, 0xFF]).vendor(), Some("KoolPOS Inc."));
        assert_eq!(registry().ma_l.get(&0x0055DA), Some(&"IEEE Registration Authority"));
    }

    #[test]
    fn real_ma_s_block_resolves_below_its_parent_oui() {
        // 001BC5 is an MA-L of the registration authority; 001BC5000 and 001BC5001 are 36-bit blocks.
        assert_eq!(MacAddress([0x00, 0x1B, 0xC5, 0x00, 0x0F, 0xFF]).vendor(), Some("Converging Systems Inc."));
        assert_eq!(MacAddress([0x00, 0x1B, 0xC5, 0x00, 0x1F, 0xFF]).vendor(), Some("OpenRB.com, Direct SIA"));
        assert_eq!(registry().ma_l.get(&0x001BC5), Some(&"IEEE Registration Authority"));
    }

    #[test]
    fn every_data_line_is_well_formed() {
        for (tsv, digits, may_be_private) in [(MA_L_TSV, 6, false), (MA_M_TSV, 7, true), (MA_S_TSV, 9, true)] {
            for line in tsv.lines() {
                let (prefix, name) = line.split_once('\t').expect("tab separated");
                assert_eq!(prefix.len(), digits, "{line}");
                assert!(u64::from_str_radix(prefix, 16).is_ok(), "{line}");
                assert!(may_be_private || !name.is_empty(), "{line}");
            }
        }
    }

    #[test]
    fn each_table_has_unique_prefixes_in_sorted_order() {
        for tsv in [MA_L_TSV, MA_M_TSV, MA_S_TSV] {
            let keys: Vec<_> = tsv.lines().map(|l| l.split('\t').next().unwrap()).collect();
            assert!(keys.windows(2).all(|w| w[0] < w[1]), "generator output is sorted and deduplicated");
        }
    }

    #[test]
    fn locally_administered_and_unknown_have_no_vendor() {
        // Same OUI bytes as VMware but with the locally-administered bit set.
        let private = MacAddress([0x02, 0x50, 0x56, 1, 2, 3]);
        assert!(private.is_locally_administered());
        assert!(private.vendor().is_none());
        assert!(!MacAddress([0x00, 0x50, 0x56, 0, 0, 0]).is_locally_administered());
    }
}
