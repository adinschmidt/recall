use clap::Parser;
use leptess::{LepTess, Variable};
use libloading::Library;
use rusqlite::{Connection, Result as SqliteResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

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

fn main() {
    let cli = Cli::parse();

    if let Err(e) = check_tesseract() {
        eprintln!("{}", e);
        process::exit(1);
    }

    let db_path = Path::new(&cli.directory).join(".ocr_results.db");
    match search_and_ocr_photos(&cli.directory, cli.debug, &db_path) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error during search and OCR: {}", e);
            process::exit(1);
        }
    }

    if let Some(search_text) = cli.search_text {
        match search_ocr_results(&db_path, &search_text) {
            Ok(results) => {
                for (filename, _) in results {
                    println!("{}", filename);
                }
            }
            Err(e) => {
                eprintln!("Error searching OCR results: {}", e);
                process::exit(1);
            }
        }
    }
}

fn check_tesseract() -> Result<(), String> {
    for lib_name in &["libtesseract", "libtesseract.dylib"] {
        if unsafe { Library::new(lib_name) }.is_ok() {
            return Ok(());
        }
    }

    Err(format!(
        "Tesseract library is missing. Please install it using:\n{}",
        r#"On Ubuntu:
            sudo apt-get install libtesseract-dev tesseract-ocr-eng

        On macOS:
            brew install tesseract

        On Windows (using vcpkg):
            vcpkg install tesseract:x64-windows"#
    ))
}

fn init_db(conn: &Connection) -> SqliteResult<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ocr_results (
            filename TEXT PRIMARY KEY,
            text TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

fn search_and_ocr_photos(
    directory: &str,
    debug: bool,
    db_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(directory);
    let mut ocr = LepTess::new(None, "eng")?;
    ocr.set_variable(Variable::TesseditPagesegMode, "1")?;

    if !debug {
        ocr.set_variable(Variable::DebugFile, "/dev/null")?;
    }

    let conn = Connection::open(db_path)?;
    init_db(&conn)?;

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if let Some(ext_str) = extension.to_str() {
                        if ["jpg", "jpeg", "png", "gif", "bmp", "tiff"]
                            .contains(&ext_str.to_lowercase().as_str())
                        {
                            if !file_exists_in_db(&conn, &path)? {
                                println!("Processing file: {}", path.display());
                                match ocr.set_image(&path) {
                                    Ok(_) => match ocr.get_utf8_text() {
                                        Ok(text) => {
                                            let trimmed_text = text.trim();
                                            if !trimmed_text.is_empty() {
                                                store_ocr_result(&conn, &path, trimmed_text)?;
                                            } else {
                                                println!("No text found in the image.");
                                            }
                                        }
                                        Err(e) => println!("Error getting OCR text: {}", e),
                                    },
                                    Err(e) => println!("Error setting image: {}", e),
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

fn file_exists_in_db(conn: &Connection, path: &Path) -> SqliteResult<bool> {
    let mut stmt = conn.prepare("SELECT 1 FROM ocr_results WHERE filename = ?1 LIMIT 1")?;
    let exists = stmt.exists([path.to_string_lossy().to_string()])?;
    Ok(exists)
}

fn store_ocr_result(conn: &Connection, path: &Path, text: &str) -> SqliteResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO ocr_results (filename, text) VALUES (?1, ?2)",
        [path.to_string_lossy().to_string(), text.to_string()],
    )?;
    Ok(())
}

fn search_ocr_results(db_path: &Path, search_text: &str) -> SqliteResult<Vec<(String, String)>> {
    let conn = Connection::open(db_path)?;
    let mut stmt =
        conn.prepare("SELECT filename, text FROM ocr_results WHERE text LIKE '%' || ?1 || '%'")?;
    let results = stmt.query_map([search_text], |row| Ok((row.get(0)?, row.get(1)?)))?;

    results.collect()
}
