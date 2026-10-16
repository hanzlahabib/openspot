//! Channel-width and center-channel resolution from HT/VHT/HE/EHT operation elements.
//! When several are present the newest generation wins (EHT > HE 6 GHz > VHT > HT).

use crate::ChannelWidth;

#[derive(Debug, Default)]
pub(crate) struct Width {
    ht: Option<Ht>,
    vht: Option<Segments>,
    he6: Option<He6>,
    eht: Option<Eht>,
}

#[derive(Debug)]
struct Ht {
    primary: u8,
    width: ChannelWidth,
}

/// VHT-style channel segment description (used by VHT and by HE's embedded copy).
#[derive(Debug, Clone, Copy)]
struct Segments {
    code: u8,
    ccfs0: u8,
    ccfs1: u8,
}

#[derive(Debug)]
struct He6 {
    primary: u8,
    segments: Segments,
}

#[derive(Debug)]
struct Eht {
    width: ChannelWidth,
    center: u8,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub width: Option<ChannelWidth>,
    pub center_channel: Option<u16>,
}

/// Bit positions inside the 24-bit HE Operation Parameters field.
const HE_VHT_INFO_PRESENT: u32 = 1 << 14;
const HE_COHOSTED_BSS_PRESENT: u32 = 1 << 15;
const HE_6GHZ_INFO_PRESENT: u32 = 1 << 17;
/// Parameters(3) + BSS color(1) + basic HE-MCS set(2).
const HE_FIXED_LEN: usize = 6;

impl Width {
    pub(crate) fn ht(&mut self, body: &[u8]) {
        let (Some(&primary), Some(&info)) = (body.first(), body.get(1)) else { return };
        let sta_width_40 = info & 0b100 != 0;
        let secondary_offset = info & 0b11;
        let width = if sta_width_40 && secondary_offset != 0 {
            ChannelWidth::Mhz40
        } else {
            ChannelWidth::Mhz20
        };
        self.ht = Some(Ht { primary, width });
    }

    pub(crate) fn vht(&mut self, body: &[u8]) {
        if let [code, ccfs0, ccfs1, ..] = *body {
            self.vht = Some(Segments { code, ccfs0, ccfs1 });
        }
    }

    pub(crate) fn he(&mut self, body: &[u8]) {
        let Some(params) = body.get(..3) else { return };
        let params = u32::from_le_bytes([params[0], params[1], params[2], 0]);
        let mut rest = match body.get(HE_FIXED_LEN..) {
            Some(r) => r,
            None => return,
        };
        if params & HE_VHT_INFO_PRESENT != 0 {
            if let [code, ccfs0, ccfs1, tail @ ..] = rest {
                self.vht = Some(Segments { code: *code, ccfs0: *ccfs0, ccfs1: *ccfs1 });
                rest = tail;
            } else {
                return;
            }
        }
        if params & HE_COHOSTED_BSS_PRESENT != 0 {
            rest = rest.get(1..).unwrap_or(&[]);
        }
        if params & HE_6GHZ_INFO_PRESENT != 0 {
            if let [primary, control, ccfs0, ccfs1, ..] = *rest {
                // 6 GHz control field: bits 0-1 width (0:20, 1:40, 2:80, 3:160/80+80).
                self.he6 = Some(He6 {
                    primary,
                    segments: Segments { code: 0x80 | (control & 0b11), ccfs0, ccfs1 },
                });
            }
        }
    }

    pub(crate) fn eht(&mut self, body: &[u8]) {
        // Parameters(1) + basic EHT-MCS set(4); operation info follows when bit 0 is set.
        let Some(&params) = body.first() else { return };
        if params & 1 == 0 {
            return;
        }
        let Some([control, ccfs0, ccfs1, ..]) = body.get(5..) else { return };
        let width = match control & 0b111 {
            0 => ChannelWidth::Mhz20,
            1 => ChannelWidth::Mhz40,
            2 => ChannelWidth::Mhz80,
            3 => ChannelWidth::Mhz160,
            4 => ChannelWidth::Mhz320,
            _ => return,
        };
        let center = if matches!(width, ChannelWidth::Mhz160 | ChannelWidth::Mhz320) { *ccfs1 } else { *ccfs0 };
        self.eht = Some(Eht { width, center });
    }

    pub(crate) fn primary_channel(&self) -> Option<u16> {
        self.he6
            .as_ref()
            .map(|h| h.primary)
            .or_else(|| self.ht.as_ref().map(|h| h.primary))
            .map(u16::from)
    }

    pub(crate) fn resolve(&self) -> Resolved {
        if let Some(e) = &self.eht {
            return Resolved { width: Some(e.width), center_channel: Some(u16::from(e.center)) };
        }
        if let Some(h) = &self.he6 {
            return h.segments.resolve();
        }
        if let Some(r) = self.vht.map(|s| s.resolve()).filter(|r| r.width.is_some()) {
            return r;
        }
        Resolved { width: self.ht.as_ref().map(|h| h.width), center_channel: None }
    }
}

impl Segments {
    /// Interprets a VHT width code (0 = defer to HT, 1 = 80/160/80+80 by CCFS, 2/3 legacy
    /// 160 and 80+80) or an HE 6 GHz code tagged with bit 7 (0:20, 1:40, 2:80, 3:wide).
    fn resolve(self) -> Resolved {
        let Self { code, ccfs0, ccfs1 } = self;
        let is_6ghz = code & 0x80 != 0;
        let code = code & 0x7F;

        let wide = |ccfs0: u8, ccfs1: u8| match (ccfs1, ccfs0.abs_diff(ccfs1)) {
            (0, _) => (ChannelWidth::Mhz80, ccfs0),
            (_, 8) => (ChannelWidth::Mhz160, ccfs1),
            _ => (ChannelWidth::Mhz80Plus80, ccfs0),
        };

        let (width, center) = match (is_6ghz, code) {
            (false, 0) => return Resolved::default(),
            (true, 0) => (ChannelWidth::Mhz20, ccfs0),
            (true, 1) => (ChannelWidth::Mhz40, ccfs0),
            (true, 2) => (ChannelWidth::Mhz80, ccfs0),
            // 6 GHz code 3 means "160 or 80+80"; ccfs1 == 0 there still implies 160 on ccfs0.
            (true, 3) if ccfs1 == 0 => (ChannelWidth::Mhz160, ccfs0),
            (_, 1) | (true, 3) => wide(ccfs0, ccfs1),
            (false, 2) => (ChannelWidth::Mhz160, ccfs0),
            (false, 3) => (ChannelWidth::Mhz80Plus80, ccfs0),
            _ => return Resolved::default(),
        };
        Resolved { width: Some(width), center_channel: Some(u16::from(center)) }
    }
}
