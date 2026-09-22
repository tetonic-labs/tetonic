//! User-data paths for capacity artifacts.

use std::path::PathBuf;

pub fn lokai_data_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "lokai").map(|d| d.data_dir().to_path_buf())
}

pub fn modelfiles_dir() -> Option<PathBuf> {
    lokai_data_dir().map(|d| d.join("inference").join("modelfiles"))
}

pub fn reports_dir() -> Option<PathBuf> {
    lokai_data_dir().map(|d| d.join("inference").join("reports"))
}
