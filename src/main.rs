//TODO
// - Clean up files that no longer exist
// - Add option to wipe database

use anyhow::{Context, Result};
use clap::Parser;
use directories::ProjectDirs;
use image::ImageReader;
use nucleo::{Config, Matcher};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod database;
mod ocr;

#[derive(Parser)]
#[command(author, version, about = "Recall is a CLI tool to OCR and search for text in your photos.", long_about = None)]
struct Cli {
    /// Text to search for in OCR results
    #[arg(index = 1)]
    search_text: Option<String>,

    /// The directory to search for photos, defaults to the current directory
    #[arg(index = 2, default_value = ".")]
    directory: PathBuf,

    /// Enable debug output
    #[arg(short, long)]
    debug: bool,

    /// Perform a search across all previously OCRed files
    #[arg(short, long)]
    global_search: bool,

    /// Number of images to process in parallel, defaults to number of CPUs
    #[arg(short, long)]
    num_threads: Option<usize>,

    /// Show credits and license information
    #[arg(long)]
    credits: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.credits {
        println!("Recall - OCR and search for text in your photos.");
        println!("Powered by ocrs models (CC-BY-SA-4.0 by Robert Knight; see https://huggingface.co/robertknight/ocrs)");
        return Ok(());
    }

    // Console layer (stdout)
    let console_layer = tracing_subscriber::fmt::layer()
        .pretty()
        .with_file(false)
        .with_level(true);
    let log_level_string = "debug".to_string();
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::Level::INFO.into())
        .parse_lossy(format!("recall={0}", &log_level_string).as_str());
    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .try_init()?;

    let Some(proj_dirs) = ProjectDirs::from("com", "adinschmidt", "recall") else {
        anyhow::bail!("Failed to get data path");
    };
    let data_path = proj_dirs.data_dir();
    fs::create_dir_all(data_path).context("Failed to create data directory")?;
    let db_path = proj_dirs.data_dir().join("data.sqlite");
    debug!("Data file path: {:?}", db_path);

    search_and_ocr_photos(&cli.directory, cli.debug, &db_path)
        .context("Error during search and OCR")?;

    if let Some(search_text) = cli.search_text {
        let results = search_ocr_results(&db_path, &search_text, &cli.directory, cli.global_search)
            .context("Error searching OCR results")?;
        for (filename, _) in results {
            println!("{}", filename);
        }
    }

    Ok(())
}

fn process_image(conn: &Connection, path: &Path) -> Result<()> {
    let image = ImageReader::open(path)
        .context("Failed to open image")?
        .decode()
        .context("Failed to decode image")?;

    let text = ocr::extract_text(&image).context("Failed to extract text from image")?;

    let trimmed_text = text.trim();
    if !trimmed_text.is_empty() {
        store_ocr_result(conn, path, trimmed_text).context("Failed to store OCR result")?;
    } else {
        info!("No text found in the image: {}", path.display());
    }

    Ok(())
}

/// Supported image extensions for OCR
const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "tiff", "webp"];

fn search_and_ocr_photos(directory: &Path, _debug: bool, db_path: &Path) -> Result<()> {
    let conn = Connection::open(db_path).context("Failed to open database connection")?;
    database::init_db(&conn)?;

    if directory.is_dir() {
        for entry_result in fs::read_dir(directory).context("Failed to read directory")? {
            let entry = entry_result.context("Failed to read directory entry")?;
            let path = entry.path();
            if path.is_file() {
                // Canonicalize path to store absolute paths
                let absolute_path = match path.canonicalize() {
                    Ok(p) => p,
                    Err(e) => {
                        error!("Failed to canonicalize path {}: {}", path.display(), e);
                        continue;
                    }
                };

                if let Some(extension) = absolute_path.extension() {
                    if let Some(ext_str) = extension.to_str() {
                        let ext_lower = ext_str.to_lowercase();
                        if SUPPORTED_EXTENSIONS.contains(&ext_lower.as_str()) {
                            if needs_ocr(&conn, &absolute_path)
                                .context("Failed to check if file needs OCR")?
                            {
                                info!("Processing file: {}", absolute_path.display());
                                if let Err(e) = process_image(&conn, &absolute_path) {
                                    error!("Error processing {}: {}", absolute_path.display(), e);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Checks if the file needs OCR
///
/// Returns true if the file does not exist in the database,
/// or if the file exists in the database but the OCR date
/// is older than the last modified date of the file.
fn needs_ocr(conn: &Connection, path: &Path) -> Result<bool> {
    let filename = path
        .file_name()
        .context("Failed to get filename from path")?
        .to_string_lossy();
    let parent_path_str = path
        .parent()
        .context("Failed to get parent path")?
        .to_string_lossy();

    let mut stmt = conn
        .prepare("SELECT 1 FROM ocr_results WHERE filename = ?1 AND path = ?2 LIMIT 1")
        .context("Failed to prepare statement for file existence check")?;
    let exists = stmt
        .exists(rusqlite::params![filename, parent_path_str])
        .context("Failed to check if file exists in database")?;
    if !exists {
        return Ok(true);
    }
    // if the file exists, check if the OCR date is older than the last modified date
    let mut stmt = conn
        .prepare("SELECT ocr_date FROM ocr_results WHERE filename = ?1 AND path = ?2 LIMIT 1")
        .context("Failed to prepare statement for file existence check")?;
    let ocr_date_str: String = stmt
        .query_row(rusqlite::params![filename, parent_path_str], |row| {
            row.get(0)
        })
        .context("Failed to get OCR date from database")?;
    let ocr_date = chrono::DateTime::parse_from_rfc3339(&ocr_date_str)
        .context("Failed to parse OCR date")?
        .with_timezone(&chrono::Utc);
    let last_modified = path
        .metadata()
        .context("Failed to get last modified date of file")?
        .modified()
        .context("Failed to get last modified date of file")?;
    let last_modified = chrono::DateTime::<chrono::Utc>::from(last_modified);
    if ocr_date < last_modified {
        return Ok(true);
    }
    Ok(false)
}

fn store_ocr_result(conn: &Connection, full_path: &Path, text: &str) -> Result<()> {
    let filename = full_path
        .file_name()
        .context("Failed to get filename from path for storing")?
        .to_string_lossy()
        .to_string();
    let parent_dir_path = full_path
        .parent()
        .context("Failed to get parent path for storing")?
        .to_string_lossy()
        .to_string();
    let ocr_date = chrono::Utc::now().to_rfc3339();
    // Assuming if this function is called, OCR was successful and text was found
    let ocr_success = true;

    conn.execute(
        "INSERT OR REPLACE INTO ocr_results (filename, path, text, ocr_date, ocr_success, ocr_engine) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![filename, parent_dir_path, text, ocr_date, ocr_success, "tesseract"],
    )
    .context("Failed to store OCR result")?;
    Ok(())
}

fn search_ocr_results(
    db_path: &Path,
    search_text: &str,
    active_directory: &Path,
    global_search: bool,
) -> Result<Vec<(String, String)>> {
    let conn = Connection::open(db_path).context("Failed to open database connection")?;
    let mut matcher = Matcher::new(Config::DEFAULT);

    let mut items_to_search: Vec<(String, String)> = Vec::new();

    if global_search {
        let mut stmt = conn
            .prepare("SELECT filename, path, text FROM ocr_results")
            .context("Failed to prepare global search statement")?;
        let mut rows = stmt
            .query([])
            .context("Failed to execute global search query")?;

        while let Some(row) = rows.next()? {
            let filename: String = row.get(0)?;
            let path_str: String = row.get(1)?;
            let text: String = row.get(2)?;
            let full_path = Path::new(&path_str).join(&filename);
            items_to_search.push((full_path.to_string_lossy().into_owned(), text));
        }
    } else {
        let canonical_active_dir = active_directory.canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize active directory: {}",
                active_directory.display()
            )
        })?;
        let active_dir_str = canonical_active_dir.to_str().ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to convert canonicalized active directory to string: {}",
                canonical_active_dir.display()
            )
        })?;

        let mut stmt = conn
            .prepare("SELECT filename, text FROM ocr_results WHERE path = ?1")
            .context("Failed to prepare directory-specific search statement")?;
        let mut rows = stmt
            .query([active_dir_str])
            .context("Failed to execute directory-specific search query")?;

        while let Some(row) = rows.next()? {
            let filename: String = row.get(0)?;
            let text: String = row.get(1)?;
            let full_path = Path::new(active_dir_str).join(&filename);
            items_to_search.push((full_path.to_string_lossy().into_owned(), text));
        }
    }

    if items_to_search.is_empty() {
        return Ok(Vec::new());
    }

    let mut ranked_results: Vec<(u16, String, String)> = Vec::new();
    // needle is &str, haystack is &str from text_content

    for (path_str, text_content) in items_to_search {
        // first convert to a utf8 str, then to a nucleo utf32 str
        // define buf: &mut Vec<char, Global> as a buffer
        let mut buf = Vec::new();
        let mut buf2 = Vec::new();
        let text_content_as_utf32 = nucleo::Utf32Str::new(&text_content, &mut buf);
        let search_text_as_utf32 = nucleo::Utf32Str::new(search_text, &mut buf2);
        if let Some(match_result) = matcher.fuzzy_match(text_content_as_utf32, search_text_as_utf32)
        {
            ranked_results.push((match_result, path_str, text_content));
        }
    }

    ranked_results.sort_by_key(|k| std::cmp::Reverse(k.0)); // Sort by score descending

    Ok(ranked_results
        .into_iter()
        .take(10) // Take top 10
        .map(|(_, path, text)| (path, text))
        .collect())
}
