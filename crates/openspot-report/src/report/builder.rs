//! Page flow: running header and footer, headings, paragraphs and paginated tables.

use crate::error::ReportError;
use crate::pdf::{Doc, PAGE_H, PAGE_W, Rgb, Weight, truncate, wrap};

pub const MARGIN: f32 = 42.0;
pub const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN;
/// Baseline of the running header text.
const HEADER_Y: f32 = 30.0;
/// First usable y below the header rule.
pub const CONTENT_TOP: f32 = 58.0;
/// Last usable y above the footer.
pub const CONTENT_BOTTOM: f32 = PAGE_H - 54.0;
const FOOTER_Y: f32 = PAGE_H - 28.0;
const ROW_H: f32 = 14.5;

pub const TEXT: Rgb = [33, 37, 41];
pub const MUTED: Rgb = [108, 117, 125];
pub const ACCENT: Rgb = [31, 95, 160];
pub const RULE: Rgb = [206, 212, 218];
const ZEBRA: Rgb = [243, 245, 247];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub title: &'static str,
    pub width: f32,
    pub align: Align,
}

impl Column {
    pub const fn left(title: &'static str, width: f32) -> Self {
        Self { title, width, align: Align::Left }
    }

    pub const fn right(title: &'static str, width: f32) -> Self {
        Self { title, width, align: Align::Right }
    }
}

#[derive(Debug)]
pub struct Builder {
    pub doc: Doc,
    pub y: f32,
    /// Left side of the running header.
    running_title: String,
    /// Right side of the running header.
    running_project: String,
    /// Page 0 exists from the start; it is handed out once before new pages are added.
    fresh: bool,
    /// Pages from this index on get a footer (the cover has none).
    footer_from: usize,
}

impl Builder {
    pub fn new(running_title: &str, running_project: &str) -> Result<Self, ReportError> {
        Ok(Self {
            doc: Doc::new()?,
            y: CONTENT_TOP,
            running_title: running_title.to_owned(),
            running_project: running_project.to_owned(),
            fresh: true,
            footer_from: 0,
        })
    }

    /// Starts the next page and returns its 0-based index (no running header; for the cover).
    pub fn start_page_bare(&mut self) -> usize {
        if self.fresh {
            self.fresh = false;
        } else {
            self.doc.new_page();
        }
        self.y = CONTENT_TOP;
        self.doc.page_count() - 1
    }

    /// Starts the next page with the running header.
    pub fn start_page(&mut self) {
        self.start_page_bare();
        let d = &mut self.doc;
        let right_budget = CONTENT_W * 0.45;
        let project = truncate(d.face(Weight::Regular), &d.sanitize(&self.running_project), 8.5, right_budget);
        d.text_right(PAGE_W - MARGIN, HEADER_Y, 8.5, Weight::Regular, MUTED, &project);
        let left_budget = CONTENT_W - d.measure(Weight::Regular, 8.5, &project) - 12.0;
        let title = truncate(d.face(Weight::Bold), &d.sanitize(&self.running_title), 8.5, left_budget);
        d.text(MARGIN, HEADER_Y, 8.5, Weight::Bold, MUTED, &title);
        d.line(MARGIN, HEADER_Y + 6.0, PAGE_W - MARGIN, HEADER_Y + 6.0, 0.5, RULE);
    }

    /// True while no page has been started.
    pub fn is_empty(&self) -> bool {
        self.fresh
    }

    pub fn set_footer_from(&mut self, page: usize) {
        self.footer_from = page;
    }

    /// Continues on a new page when less than `height` remains.
    pub fn ensure(&mut self, height: f32) -> bool {
        if self.y + height > CONTENT_BOTTOM {
            self.start_page();
            return true;
        }
        false
    }

    pub fn heading(&mut self, text: &str, size: f32) {
        self.ensure(size * 2.4);
        self.y += size;
        self.doc.text(MARGIN, self.y, size, Weight::Bold, TEXT, text);
        self.y += size * 0.7;
    }

    /// Wrapped paragraph in the full content width; may continue on later pages.
    pub fn paragraph(&mut self, text: &str, size: f32, color: Rgb) {
        let lead = size * 1.4;
        let clean = self.doc.sanitize(text);
        for line in wrap(self.doc.face(Weight::Regular), &clean, size, CONTENT_W) {
            self.ensure(lead);
            self.y += lead;
            self.doc.text(MARGIN, self.y, size, Weight::Regular, color, &line);
        }
        self.y += lead * 0.4;
    }

    pub fn gap(&mut self, h: f32) {
        self.y += h;
    }

    /// Draws a table with a repeated header row; breaks across pages between rows.
    pub fn table(&mut self, cols: &[Column], rows: &[Vec<String>]) {
        const SIZE: f32 = 8.5;
        const PAD: f32 = 3.0;
        self.header_row(cols, SIZE, PAD);
        for (i, row) in rows.iter().enumerate() {
            if self.y + ROW_H > CONTENT_BOTTOM {
                self.start_page();
                self.header_row(cols, SIZE, PAD);
            }
            if i % 2 == 1 {
                self.doc.fill_rect(MARGIN, self.y, CONTENT_W, ROW_H, ZEBRA);
            }
            let base = self.y + ROW_H - 4.5;
            let mut x = MARGIN;
            for (col, cell) in cols.iter().zip(row) {
                let clean = self.doc.sanitize(cell);
                let fitted = truncate(self.doc.face(Weight::Regular), &clean, SIZE, col.width - 2.0 * PAD);
                match col.align {
                    Align::Left => self.doc.text(x + PAD, base, SIZE, Weight::Regular, TEXT, &fitted),
                    Align::Right => self.doc.text_right(x + col.width - PAD, base, SIZE, Weight::Regular, TEXT, &fitted),
                };
                x += col.width;
            }
            self.y += ROW_H;
        }
        self.doc.line(MARGIN, self.y, PAGE_W - MARGIN, self.y, 0.5, RULE);
        self.y += 6.0;
    }

    fn header_row(&mut self, cols: &[Column], size: f32, pad: f32) {
        self.doc.fill_rect(MARGIN, self.y, CONTENT_W, ROW_H, [229, 236, 245]);
        let base = self.y + ROW_H - 4.5;
        let mut x = MARGIN;
        for col in cols {
            match col.align {
                Align::Left => self.doc.text(x + pad, base, size, Weight::Bold, TEXT, col.title),
                Align::Right => self.doc.text_right(x + col.width - pad, base, size, Weight::Bold, TEXT, col.title),
            };
            x += col.width;
        }
        self.y += ROW_H;
    }

    /// Adds "Page n of N" to every page from `footer_from`, and consumes the builder.
    pub fn finish_pages(mut self, footer_left: &str) -> Doc {
        let total = self.doc.page_count();
        for i in self.footer_from..total {
            self.doc.select_page(i);
            self.doc.line(MARGIN, FOOTER_Y - 9.0, PAGE_W - MARGIN, FOOTER_Y - 9.0, 0.5, RULE);
            self.doc.text(MARGIN, FOOTER_Y, 8.0, Weight::Regular, MUTED, footer_left);
            self.doc.text_right(PAGE_W - MARGIN, FOOTER_Y, 8.0, Weight::Regular, MUTED, &format!("Page {} of {total}", i + 1));
        }
        self.doc
    }
}
