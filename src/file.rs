use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, ffi::OsString, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SupportedFileType {
    Avif,
    Bmp,
    Gif,
    Ico,
    Jpeg,
    Png,
    Webp,
    Heic,
    Jxl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    /// the filename
    pub name: OsString,
    /// the base path of the file
    pub path: PathBuf,
    /// the OCRed text
    pub text: String,
    /// the date the file was OCRed
    pub ocr_date: DateTime<chrono::Utc>,
    /// whether OCR was successful
    pub ocr_success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Data {
    // a hashmap of the OCRed text : the filename
    pub ocr_results: HashMap<String, OsString>,
    // a hashmap of the (path + filename) : the file data
    pub files: HashMap<String, File>,
}
