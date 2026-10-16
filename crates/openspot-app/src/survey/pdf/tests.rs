use super::*;

/// Builds a PDF with one page per `(width_pt, height_pt, content_stream)`, with a correct
/// cross-reference table.
pub(crate) fn minimal_pdf(pages: &[(u32, u32, &str)]) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 3 + 2 * i)).collect();
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()));
    for (i, (w, h, content)) in pages.iter().enumerate() {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents {} 0 R >>", 4 + 2 * i));
        objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

fn pixel(png: &[u8], x: u32, y: u32) -> [u8; 4] {
    image::load_from_memory(png).unwrap().to_rgba8().get_pixel(x, y).0
}

#[test]
fn red_rectangle_lands_where_the_pdf_puts_it_with_y_flipped() {
    // 200 x 100 pt page, red rectangle from (20,20) to (80,60) in PDF space (origin bottom-left).
    let pdf = minimal_pdf(&[(200, 100, "1 0 0 rg 20 20 60 40 re f")]);
    let page = render_page(pdf, 0).unwrap();
    let scale = RENDER_DPI / 72.0;
    assert_eq!((page.width, page.height), ((200.0 * scale).floor() as u32, (100.0 * scale).floor() as u32));
    assert_eq!(page.page_count, 1);
    let px = |x_pt: f32, y_from_top_pt: f32| pixel(&page.png, (x_pt * scale) as u32, (y_from_top_pt * scale) as u32);
    // PDF y 20..60 is 40..80 pt from the top.
    assert_eq!(px(50.0, 60.0), [255, 0, 0, 255], "inside the rectangle");
    assert_eq!(px(150.0, 60.0), [255, 255, 255, 255], "right of it");
    assert_eq!(px(50.0, 10.0), [255, 255, 255, 255], "above it (PDF y 90)");
    assert_eq!(px(50.0, 90.0), [255, 255, 255, 255], "below it (PDF y 10)");
}

#[test]
fn page_chooser_renders_the_requested_page() {
    let pdf = minimal_pdf(&[(100, 100, "1 0 0 rg 0 0 100 100 re f"), (100, 100, "0 0 1 rg 0 0 100 100 re f")]);
    assert_eq!(page_count(&pdf), Ok(2));
    let first = render_page(pdf.clone(), 0).unwrap();
    let second = render_page(pdf.clone(), 1).unwrap();
    assert_eq!(pixel(&first.png, 10, 10), [255, 0, 0, 255]);
    assert_eq!(pixel(&second.png, 10, 10), [0, 0, 255, 255]);
    assert_eq!(render_page(pdf, 2), Err(PdfError::NoSuchPage { requested: 3, count: 2 }));
}

#[test]
fn large_sheets_are_capped_to_the_maximum_edge() {
    // A0-ish 2384 x 3370 pt would be 4857 x 6865 px at 150 dpi.
    let s = render_scale(2384.0, 3370.0);
    assert!((3370.0 * s - MAX_RENDER_SIDE_PX).abs() < 0.01);
    assert!(s < RENDER_DPI / 72.0);
    // Letter stays at full resolution.
    assert_eq!(render_scale(612.0, 792.0), RENDER_DPI / 72.0);
    let pdf = minimal_pdf(&[(2384, 3370, "0 g 100 100 400 400 re f")]);
    let page = render_page(pdf, 0).unwrap();
    assert!(page.width.max(page.height) <= MAX_RENDER_SIDE_PX as u32);
    assert_eq!(page.height, MAX_RENDER_SIDE_PX as u32);
}

#[test]
fn garbage_and_empty_input_are_errors_not_blank_plans() {
    assert_eq!(render_page(b"definitely not a pdf".to_vec(), 0), Err(PdfError::Invalid));
    assert_eq!(render_page(Vec::new(), 0), Err(PdfError::Invalid));
    assert_eq!(page_count(b"%PDF-1.4 truncated"), Err(PdfError::Invalid));
}
