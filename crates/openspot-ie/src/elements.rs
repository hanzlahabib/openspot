//! Iteration over the `id | length | body` records of an IE run.

pub(crate) const DS_PARAMETER: u8 = 3;
pub(crate) const SUPPORTED_RATES: u8 = 1;
pub(crate) const EXTENDED_RATES: u8 = 50;
pub(crate) const RSN: u8 = 48;
pub(crate) const HT_CAPABILITIES: u8 = 45;
pub(crate) const HT_OPERATION: u8 = 61;
pub(crate) const VHT_CAPABILITIES: u8 = 191;
pub(crate) const VHT_OPERATION: u8 = 192;
pub(crate) const VENDOR_SPECIFIC: u8 = 221;
pub(crate) const EXTENSION: u8 = 255;

pub(crate) const EXT_HE_CAPABILITIES: u8 = 35;
pub(crate) const EXT_HE_OPERATION: u8 = 36;
pub(crate) const EXT_EHT_OPERATION: u8 = 106;
pub(crate) const EXT_EHT_CAPABILITIES: u8 = 108;

/// One information element. For the Extension element, `body` excludes the
/// leading extension-id byte (see [`Element::ext_id`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Element<'a> {
    pub id: u8,
    pub body: &'a [u8],
    ext_id: Option<u8>,
}

impl Element<'_> {
    /// Element ID Extension, present only when `id == 255`.
    pub fn ext_id(&self) -> Option<u8> {
        self.ext_id
    }
}

/// Iterator over well-formed elements; stops at the first truncated one.
#[derive(Debug, Clone)]
pub struct Elements<'a> {
    rest: &'a [u8],
}

impl<'a> Elements<'a> {
    pub fn new(ies: &'a [u8]) -> Self {
        Self { rest: ies }
    }
}

impl<'a> Iterator for Elements<'a> {
    type Item = Element<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (&id, after_id) = self.rest.split_first()?;
        let (&len, after_len) = after_id.split_first()?;
        let body = after_len.get(..usize::from(len))?;
        self.rest = &after_len[usize::from(len)..];

        if id == EXTENSION {
            let (&ext, ext_body) = body.split_first()?;
            return Some(Element { id, body: ext_body, ext_id: Some(ext) });
        }
        Some(Element { id, body, ext_id: None })
    }
}
