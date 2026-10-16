use super::*;
use crate::survey::pdf::tests::minimal_pdf;

fn temp_file(name: &str, bytes: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("openspot-pdf-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, bytes).unwrap();
    (dir, file)
}

#[test]
fn single_page_pdf_loads_straight_into_the_zone() {
    let (dir, file) = temp_file("Plan.PDF", &minimal_pdf(&[(200, 100, "0 0 1 rg 0 0 200 100 re f")]));
    let mut s = Survey::default();
    s.set_scale(Scale::new(5.0).unwrap());
    s.load_plan(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    let map = s.zone().map.as_ref().unwrap();
    assert_eq!(map.original_name, "Plan.PDF");
    assert_eq!((map.width, map.height), (416, 208));
    assert!(s.pdf_choice.is_none());
    assert!(s.zone().scale.is_none(), "a new plan needs calibrating");
    let img = image::load_from_memory(&map.data).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(10, 10).0, [0, 0, 255, 255]);
}

#[test]
fn multi_page_pdf_asks_which_page_and_loads_the_chosen_one() {
    let pdf = minimal_pdf(&[(100, 100, "1 0 0 rg 0 0 100 100 re f"), (100, 100, "0 1 0 rg 0 0 100 100 re f")]);
    let (dir, file) = temp_file("two.pdf", &pdf);
    let mut s = Survey::default();
    s.load_plan(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    assert!(s.zone().map.is_none(), "nothing loaded before the user chooses");
    let choice = s.pdf_choice.as_mut().unwrap();
    assert_eq!(choice.page_count, 2);
    choice.page = 2;
    s.confirm_pdf_choice().unwrap();
    let map = s.zone().map.as_ref().unwrap();
    assert_eq!(map.original_name, "two.pdf (page 2)");
    assert_eq!(image::load_from_memory(&map.data).unwrap().to_rgba8().get_pixel(5, 5).0, [0, 255, 0, 255]);
    assert!(s.pdf_choice.is_none());
}

#[test]
fn bad_pdfs_report_errors_and_leave_the_current_plan_alone() {
    let (dir, file) = temp_file("bad.pdf", b"%PDF-1.4 nope");
    let mut s = Survey::default();
    s.create_blank_map(10.0, 10.0).unwrap();
    let before = s.zone().map.clone();
    assert!(s.load_plan(&file).is_err());
    assert!(s.load_plan(&dir.join("missing.pdf")).is_err());
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    assert_eq!(s.zone().map, before);
    assert!(s.pdf_choice.is_none());
}

#[test]
fn images_still_load_through_load_plan() {
    let (dir, file) = temp_file("p.png", &{
        let mut c = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(30, 20).write_to(&mut c, image::ImageFormat::Png).unwrap();
        c.into_inner()
    });
    let mut s = Survey::default();
    s.load_plan(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    assert_eq!(s.zone().map.as_ref().unwrap().width, 30);
}

#[test]
fn a_pdf_plan_replaces_a_drawn_plan() {
    let (dir, file) = temp_file("p.pdf", &minimal_pdf(&[(50, 50, "0 g 0 0 10 10 re f")]));
    let mut s = Survey::default();
    s.draw_new_plan(10.0, 10.0).unwrap();
    s.load_plan(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir); // best effort: Windows may still hold a handle
    assert!(s.drawing().is_none());
}
