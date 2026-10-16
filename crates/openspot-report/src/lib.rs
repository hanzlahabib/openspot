//! Reports and interchange for OpenSpot: PDF survey reports, per-AP heatmap export and
//! project archives.

#![forbid(unsafe_code)]

mod archive;
mod branding;
mod error;
mod names;
mod options;
mod per_ap;
mod pdf;
mod report;
#[cfg(test)]
mod testdata;
#[cfg(test)]
mod testpdf;
mod timefmt;

pub use archive::{ARCHIVE_EXTENSION, ArchiveError, FORMAT_VERSION, Limits, MAX_DATABASE_BYTES, Manifest, Member, export_project, extract_database, import_project, write_archive};
pub use branding::ReportBranding;
pub use error::ReportError;
pub use names::{MAX_COMPONENT_BYTES, sanitize_file_name};
pub use options::{DEFAULT_MAX_AP_ROWS, MIN_USABLE_RSSI_DBM, ReportKind, ReportOptions, Sections, Titles, ViewOptions, Visualization};
pub use per_ap::{PerApExport, export_per_ap_heatmaps, file_name as per_ap_file_name};
pub use report::{NetworkRow, NoObserver, Observer, ReportData, generate_pdf, network_rows};
