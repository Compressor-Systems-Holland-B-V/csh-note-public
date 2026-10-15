//! Packs a folder of pages into an encrypted .znote file, for the demo recording
//! (scripts/record.sh). The app only makes these behind a native save dialog,
//! which the recording cannot answer. The layout follows src/snapshot.rs.
//!
//!     cargo run --example demo_znote -- PAGES OUT PASSWORD
use std::error::Error;
use std::fs;
use std::path::Path;

use rusqlite::{Connection, params};

const MANIFEST: &str = r#"{"format": 2, "look": null, "themes": [], "settings": {}}"#;

fn add(db: &Connection, root: &Path, folder: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path.is_dir() {
            add(db, root, &path)?;
        } else {
            let name = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            db.execute(
                "INSERT INTO entries (name, data) VALUES (?1, ?2)",
                params![format!("pages/{name}"), fs::read(&path)?],
            )?;
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let [_, pages, out, password] = <[String; 4]>::try_from(std::env::args().collect::<Vec<_>>())
        .map_err(|_| "usage: demo_znote PAGES OUT PASSWORD")?;
    let _ = fs::remove_file(&out);
    let db = Connection::open(&out)?;
    db.pragma_update(None, "key", &password)?;
    db.execute_batch("CREATE TABLE entries (name TEXT PRIMARY KEY, data BLOB NOT NULL);")?;
    add(&db, Path::new(&pages), Path::new(&pages))?;
    db.execute(
        "INSERT INTO entries (name, data) VALUES ('znote.json', ?1)",
        [MANIFEST.as_bytes()],
    )?;
    println!("==> {out}");
    Ok(())
}
