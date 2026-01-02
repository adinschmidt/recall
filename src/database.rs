use anyhow::{Context, Result};
use rusqlite::Connection;

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ocr_results (
            filename TEXT NOT NULL,
            path TEXT NOT NULL,
            text TEXT NOT NULL,
            ocr_date TEXT NOT NULL,
            ocr_success BOOLEAN NOT NULL,
            ocr_engine TEXT NOT NULL,
            PRIMARY KEY (filename, path)
        )",
        [],
    )
    .context("Failed to create table ocr_results")?;
    Ok(())
}
