//! Survey-level PDF loading.

use std::path::Path;

use openspot_model::MapImage;

use super::{PdfError, page_count, render_page};
use crate::survey::Survey;

/// A PDF with more than one page waiting for the user to choose which to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfChoice {
    pub name: String,
    pub data: Vec<u8>,
    pub page_count: usize,
    /// 1-based page shown in the dialog.
    pub page: usize,
}

fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

impl Survey {
    /// Loads a floor plan from an image or PDF. A multi-page PDF opens the page chooser
    /// instead of loading right away.
    pub fn load_plan(&mut self, path: &Path) -> Result<(), String> {
        if !is_pdf(path) {
            return self.load_map(path);
        }
        let data = std::fs::read(path).map_err(|e| PdfError::Read(format!("{}: {e}", path.display())).to_string())?;
        let name = path.file_name().map_or_else(|| "plan.pdf".into(), |n| n.to_string_lossy().into_owned());
        let count = page_count(&data).map_err(|e| e.to_string())?;
        if count == 0 {
            return Err(PdfError::NoPages.to_string());
        }
        if count == 1 {
            return self.load_pdf_page(&name, data, 1);
        }
        self.pdf_choice = Some(PdfChoice { name, data, page_count: count, page: 1 });
        Ok(())
    }

    /// Renders `page` (1-based) and uses it as the current zone's floor plan.
    pub fn load_pdf_page(&mut self, name: &str, data: Vec<u8>, page: usize) -> Result<(), String> {
        let rendered = render_page(data, page.saturating_sub(1)).map_err(|e| e.to_string())?;
        let original_name = if rendered.page_count > 1 { format!("{name} (page {page})") } else { name.to_owned() };
        self.install_map(MapImage { original_name, data: rendered.png, width: rendered.width, height: rendered.height });
        Ok(())
    }

    /// Confirms the page chosen in the dialog.
    pub fn confirm_pdf_choice(&mut self) -> Result<(), String> {
        let Some(choice) = self.pdf_choice.take() else { return Ok(()) };
        self.load_pdf_page(&choice.name, choice.data, choice.page)
    }
}
