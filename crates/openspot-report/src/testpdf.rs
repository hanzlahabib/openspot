//! Test-only PDF inspection built on `lopdf`, independent of the writer under test.

use std::collections::BTreeMap;

use lopdf::content::Content;
use lopdf::{Document, Object, ObjectId};

pub struct Parsed {
    pub doc: Document,
}

fn name_key(k: &[u8]) -> Vec<u8> {
    k.to_vec()
}

/// Parses `<gid> <unicode>` pairs of a `ToUnicode` CMap's `bfchar` sections.
fn parse_cmap(text: &str) -> BTreeMap<u16, char> {
    let mut map = BTreeMap::new();
    let mut in_chars = false;
    for line in text.lines() {
        let line = line.trim();
        if line.ends_with("beginbfchar") {
            in_chars = true;
        } else if line == "endbfchar" {
            in_chars = false;
        } else if in_chars {
            let parts: Vec<&str> = line.split(['<', '>']).map(str::trim).filter(|s| !s.is_empty()).collect();
            if let [gid, uni] = parts[..] {
                let g = u16::from_str_radix(gid, 16).unwrap();
                let u = u32::from_str_radix(uni, 16).unwrap();
                map.insert(g, char::from_u32(u).unwrap());
            }
        }
    }
    map
}

impl Parsed {
    pub fn new(bytes: &[u8]) -> Self {
        Self { doc: Document::load_mem(bytes).expect("generated PDF must parse") }
    }

    pub fn page_ids(&self) -> Vec<ObjectId> {
        self.doc.get_pages().into_values().collect()
    }

    pub fn page_count(&self) -> usize {
        self.page_ids().len()
    }

    fn font_maps(&self, page: ObjectId) -> BTreeMap<Vec<u8>, BTreeMap<u16, char>> {
        let mut out = BTreeMap::new();
        for (name, dict) in self.doc.get_page_fonts(page).unwrap() {
            let to_uni = dict.get(b"ToUnicode").unwrap().as_reference().unwrap();
            let stream = self.doc.get_object(to_uni).unwrap().as_stream().unwrap();
            let text = String::from_utf8(stream.decompressed_content().unwrap()).unwrap();
            out.insert(name_key(&name), parse_cmap(&text));
        }
        out
    }

    /// Text shown on each page (in drawing order, one entry per `Tj`), decoded through the
    /// fonts' `ToUnicode` maps exactly as a text extractor would.
    pub fn page_runs(&self, page: ObjectId) -> Vec<String> {
        let maps = self.font_maps(page);
        let content = Content::decode(&self.doc.get_page_content(page)).unwrap();
        let mut font = Vec::new();
        let mut runs = Vec::new();
        for op in content.operations {
            match op.operator.as_str() {
                "Tf" => font = op.operands[0].as_name().unwrap().to_vec(),
                "Tj" => {
                    let bytes = op.operands[0].as_str().unwrap();
                    let map = &maps[&font];
                    runs.push(bytes.chunks(2).map(|p| map[&u16::from_be_bytes([p[0], p[1]])]).collect());
                }
                _ => {}
            }
        }
        runs
    }

    pub fn all_runs(&self) -> Vec<String> {
        self.page_ids().into_iter().flat_map(|p| self.page_runs(p)).collect()
    }

    pub fn page_text(&self, index: usize) -> String {
        self.page_runs(self.page_ids()[index]).join("\n")
    }

    /// Number of drawing operators of a given kind on a page, e.g. `"Do"` (image draws).
    pub fn op_count(&self, page: ObjectId, operator: &str) -> usize {
        Content::decode(&self.doc.get_page_content(page)).unwrap().operations.iter().filter(|o| o.operator == operator).count()
    }

    /// Image XObjects in the file (alpha masks included).
    pub fn image_objects(&self) -> Vec<&lopdf::Stream> {
        self.doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| s.dict.get(b"Subtype").ok().and_then(|n| n.as_name().ok()) == Some(b"Image"))
            .collect()
    }

    /// A string entry of the document information dictionary.
    pub fn info(&self, key: &[u8]) -> Option<String> {
        let info = self.doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
        let dict = self.doc.get_object(info).ok()?.as_dict().ok()?;
        match dict.get(key).ok()? {
            Object::String(bytes, _) => Some(decode_text_string(bytes)),
            _ => None,
        }
    }
}

fn decode_text_string(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.chunks(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
        String::from_utf16(&units).unwrap()
    } else {
        bytes.iter().map(|&b| char::from(b)).collect()
    }
}
