use anyhow::{Context, Result};
use clap::Parser;
use image::{DynamicImage, ImageFormat, ImageReader};
use leptess::{LepTess, Variable};
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use tempfile::Builder;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// The directory to search for photos
    #[arg(value_name = "DIRECTORY")]
    directory: String,

    /// Text to search for in OCR results
    #[arg(value_name = "SEARCH_TEXT")]
    search_text: Option<String>,

    /// Enable debug output
    #[arg(short, long)]
    debug: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let db_path = Path::new(&cli.directory).join(".ocr_results.db");
    search_and_ocr_photos(&cli.directory, cli.debug, &db_path)
        .context("Error during search and OCR")?;

    if let Some(search_text) = cli.search_text {
        let results =
            search_ocr_results(&db_path, &search_text).context("Error searching OCR results")?;
        for (filename, _) in results {
            println!("{}", filename);
        }
    }

    Ok(())
}

fn init_db(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ocr_results (
            filename TEXT PRIMARY KEY,
            text TEXT NOT NULL
        )",
        [],
    )
    .context("Failed to create table")?;
    Ok(())
}

fn process_image(
    ocr: &mut LepTess,
    conn: &Connection,
    path: &Path,
    image: Option<DynamicImage>,
) -> Result<()> {
    let temp_file = Builder::new()
        .suffix(".png")
        .tempfile()
        .context("Failed to create temporary file")?;
    let temp_path = temp_file.path();

    if let Some(img) = image {
        img.save_with_format(temp_path, ImageFormat::Png)
            .context("Failed to save image to temporary file")?;
    } else {
        fs::copy(path, temp_path).context("Failed to copy image to temporary file")?;
    }

    ocr.set_image(temp_path)
        .context("Failed to set image for OCR")?;

    let text = ocr.get_utf8_text().context("Failed to get OCR text")?;

    let trimmed_text = text.trim();
    if !trimmed_text.is_empty() {
        store_ocr_result(conn, path, trimmed_text).context("Failed to store OCR result")?;
    } else {
        println!("No text found in the image.");
    }

    Ok(())
}

fn search_and_ocr_photos(directory: &str, debug: bool, db_path: &Path) -> Result<()> {
    let path = Path::new(directory);
    let mut ocr = LepTess::new(None, "eng").context("Failed to initialize LepTess")?;
    ocr.set_variable(Variable::TesseditPagesegMode, "1")
        .context("Failed to set TesseditPagesegMode")?;

    if !debug {
        ocr.set_variable(Variable::DebugFile, "/dev/null")
            .context("Failed to set DebugFile")?;
    }

    let conn = Connection::open(db_path).context("Failed to open database connection")?;
    init_db(&conn)?;

    if path.is_dir() {
        for entry in fs::read_dir(path).context("Failed to read directory")? {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if let Some(ext_str) = extension.to_str() {
                        let ext_lower = ext_str.to_lowercase();
                        if ["jpg", "jpeg", "png", "gif", "bmp", "tiff"]
                            .contains(&ext_lower.as_str())
                        {
                            if !file_exists_in_db(&conn, &path)
                                .context("Failed to check if file exists in database")?
                            {
                                println!("Processing file: {}", path.display());
                                process_image(&mut ocr, &conn, &path, None)?;
                            }
                        } else if ["webp", "heic", "heif", "avif", "jxl"]
                            .contains(&ext_lower.as_str())
                        {
                            if !file_exists_in_db(&conn, &path)
                                .context("Failed to check if file exists in database")?
                            {
                                println!("Processing file: {}", path.display());
                                let image = ImageReader::open(&path)
                                    .context("Failed to open image")?
                                    .decode()
                                    .context("Failed to decode image")?;
                                process_image(&mut ocr, &conn, &path, Some(image))?;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn file_exists_in_db(conn: &Connection, path: &Path) -> Result<bool> {
    let mut stmt = conn
        .prepare("SELECT 1 FROM ocr_results WHERE filename = ?1 LIMIT 1")
        .context("Failed to prepare statement")?;
    let exists = stmt
        .exists([path.to_string_lossy().to_string()])
        .context("Failed to check if file exists in database")?;
    Ok(exists)
}

fn store_ocr_result(conn: &Connection, path: &Path, text: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO ocr_results (filename, text) VALUES (?1, ?2)",
        [path.to_string_lossy().to_string(), text.to_string()],
    )
    .context("Failed to store OCR result")?;
    Ok(())
}

fn search_ocr_results(db_path: &Path, search_text: &str) -> Result<Vec<(String, String)>> {
    let conn = Connection::open(db_path).context("Failed to open database connection")?;
    let mut stmt = conn
        .prepare("SELECT filename, text FROM ocr_results WHERE text LIKE '%' || ?1 || '%'")
        .context("Failed to prepare statement")?;
    let results = stmt
        .query_map([search_text], |row| Ok((row.get(0)?, row.get(1)?)))
        .context("Failed to execute query")?;

    results
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to collect results")
}
