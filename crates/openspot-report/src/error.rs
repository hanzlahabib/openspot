use std::path::PathBuf;

use openspot_export::ExportError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("report generation was cancelled")]
    Cancelled,
    #[error("could not render the heatmap of zone '{zone}': {source}")]
    Heatmap {
        zone: String,
        #[source]
        source: ExportError,
    },
    #[error("could not use the company logo: {0}")]
    Logo(#[source] image::ImageError),
    #[error("could not process an image for the report: {0}")]
    Image(#[from] image::ImageError),
    #[error("the embedded report font is unusable: {0}")]
    Font(#[from] ttf_parser::FaceParsingError),
    #[error("could not write {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
