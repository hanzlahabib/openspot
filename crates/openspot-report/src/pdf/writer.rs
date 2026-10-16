//! Serialises a [`Doc`] into PDF bytes.

use pdf_writer::types::{CidFontType, FontFlags, SystemInfo};
use pdf_writer::{Date, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr, writers::Resources};
use pdf_writer::types::UnicodeCmap;
use sha2::{Digest, Sha256};

use super::{Doc, FontState, PAGE_H, PAGE_W};
use crate::timefmt::civil_from_unix;

/// zlib effort 6 is miniz's default trade-off; the output is deterministic at any level.
const DEFLATE_LEVEL: u8 = 6;

/// Document information dictionary values.
#[derive(Debug, Clone)]
pub struct Metadata<'a> {
    pub title: &'a str,
    pub author: &'a str,
    pub subject: &'a str,
    pub keywords: &'a str,
    pub creator: &'a str,
    pub producer: &'a str,
    /// Creation time, Unix seconds (injected so output is reproducible).
    pub created_unix: i64,
}

pub(super) fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, DEFLATE_LEVEL)
}

struct Ids {
    next: i32,
}

impl Ids {
    fn alloc(&mut self) -> Ref {
        let r = Ref::new(self.next);
        self.next += 1;
        r
    }
}

fn pdf_date(unix: i64) -> Date {
    let c = civil_from_unix(unix);
    Date::new(c.year.clamp(0, 9999) as u16)
        .month(c.month)
        .day(c.day)
        .hour(c.hour)
        .minute(c.minute)
        .second(c.second)
        .utc_offset_hour(0)
}

impl Doc {
    /// Finishes the document. Consumes it: pages cannot be changed afterwards.
    pub fn finish(self, meta: &Metadata<'_>) -> Vec<u8> {
        let mut pdf = Pdf::new();
        let mut ids = Ids { next: 1 };
        let catalog_id = ids.alloc();
        let pages_id = ids.alloc();
        let info_id = ids.alloc();

        // Fonts: only those that drew text.
        let mut font_refs: Vec<(&'static [u8], Ref)> = Vec::new();
        for (i, state) in self.fonts.iter().enumerate() {
            if state.used.is_empty() {
                continue;
            }
            let id = write_font(&mut pdf, &mut ids, state, if i == 0 { "NotoSans-Regular" } else { "NotoSans-Bold" });
            font_refs.push((if i == 0 { b"F1" } else { b"F2" }, id));
        }

        // Images (and their alpha masks).
        let image_refs: Vec<Ref> = self
            .images
            .iter()
            .map(|img| {
                let id = ids.alloc();
                let mask = img.alpha.as_ref().map(|alpha| {
                    let mid = ids.alloc();
                    let mut m = pdf.image_xobject(mid, alpha);
                    m.filter(Filter::FlateDecode);
                    m.width(img.width as i32).height(img.height as i32);
                    m.color_space().device_gray();
                    m.bits_per_component(8);
                    m.finish();
                    mid
                });
                let mut x = pdf.image_xobject(id, &img.rgb);
                x.filter(Filter::FlateDecode);
                x.width(img.width as i32).height(img.height as i32);
                x.color_space().device_rgb();
                x.bits_per_component(8);
                if let Some(mid) = mask {
                    x.s_mask(mid);
                }
                x.finish();
                id
            })
            .collect();

        // Pages.
        let mut hasher = Sha256::new();
        let mut page_ids = Vec::new();
        for page in self.pages {
            let page_id = ids.alloc();
            let content_id = ids.alloc();
            let content = page.content.finish();
            let compressed = deflate(&content);
            hasher.update(&compressed);
            pdf.stream(content_id, &compressed).filter(Filter::FlateDecode);

            let mut p = pdf.page(page_id);
            p.media_box(Rect::new(0.0, 0.0, PAGE_W, PAGE_H)).parent(pages_id).contents(content_id);
            let mut res: Resources = p.resources();
            {
                let mut fonts = res.fonts();
                for (name, id) in &font_refs {
                    fonts.pair(Name(name), *id);
                }
            }
            if !page.images.is_empty() {
                let mut xo = res.x_objects();
                for &i in &page.images {
                    xo.pair(Name(format!("Im{i}").as_bytes()), image_refs[i]);
                }
            }
            res.finish();
            p.finish();
            page_ids.push(page_id);
        }

        pdf.catalog(catalog_id).pages(pages_id);
        pdf.pages(pages_id).kids(page_ids.iter().copied()).count(page_ids.len() as i32);
        let mut info = pdf.document_info(info_id);
        info.title(TextStr(meta.title)).author(TextStr(meta.author)).subject(TextStr(meta.subject));
        info.keywords(TextStr(meta.keywords)).creator(TextStr(meta.creator)).producer(TextStr(meta.producer));
        info.creation_date(pdf_date(meta.created_unix));
        info.finish();

        // A content-derived file identifier keeps identical reports byte-identical.
        for s in [meta.title, meta.author, meta.subject] {
            hasher.update(s.as_bytes());
        }
        hasher.update(meta.created_unix.to_be_bytes());
        let digest = hasher.finalize();
        pdf.set_file_id((digest[..16].to_vec(), digest[..16].to_vec()));
        pdf.finish()
    }
}

/// Writes a Type0 composite font with an embedded TrueType descendant and returns the
/// reference to the Type0 dictionary.
fn write_font(pdf: &mut Pdf, ids: &mut Ids, state: &FontState, base_name: &str) -> Ref {
    let (type0, cid, descriptor, file, to_unicode) = (ids.alloc(), ids.alloc(), ids.alloc(), ids.alloc(), ids.alloc());
    let base = Name(base_name.as_bytes());
    let system = SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 };

    pdf.type0_font(type0)
        .base_font(base)
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(cid)
        .to_unicode(to_unicode);

    {
        let mut f = pdf.cid_font(cid);
        f.subtype(CidFontType::Type2).base_font(base).system_info(system).font_descriptor(descriptor);
        f.cid_to_gid_map_predefined(Name(b"Identity"));
        let mut w = f.widths();
        for &gid in state.used.keys() {
            let ch = state.used[&gid];
            w.consecutive(gid, [state.face.advance_1000(ch)]);
        }
        w.finish();
        f.finish();
    }

    let (ascent, descent, cap, bbox) = state.face.metrics_1000();
    pdf.font_descriptor(descriptor)
        .name(base)
        .flags(FontFlags::NON_SYMBOLIC)
        .bbox(Rect::new(bbox[0], bbox[1], bbox[2], bbox[3]))
        .italic_angle(0.0)
        .ascent(ascent)
        .descent(descent)
        .cap_height(cap)
        // StemV is required but unused for embedded fonts; 80 is a typical regular-weight value.
        .stem_v(80.0)
        .font_file2(file);

    let data = state.face.data();
    pdf.stream(file, &deflate(data)).filter(Filter::FlateDecode).pair(Name(b"Length1"), data.len() as i32);

    let mut cmap = UnicodeCmap::new(Name(b"Custom"), SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"UCS"), supplement: 0 });
    for (&gid, &ch) in &state.used {
        cmap.pair(gid, ch);
    }
    pdf.cmap(to_unicode, &cmap.finish());
    type0
}
