use std::path::Path;

use keyring::{Entry, Error};
use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags};

pub const MIN_PASSWORD: usize = 8;
const FOREIGN: &str = "This file holds more than znote puts in one, so it is not opened.";

pub fn open(
    file: &Path,
    password: &str,
    flags: OpenFlags,
    wrong: &str,
) -> Result<Connection, String> {
    let failed = |error: rusqlite::Error| format!("The file could not be used: {error}");
    let db = Connection::open_with_flags(file, flags).map_err(failed)?;
    let _ = db.pragma_update(None, "cipher_log_level", "NONE");
    db.pragma_update(None, "key", password).map_err(failed)?;
    db.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|_| wrong.to_string())?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(failed)?;
    db.pragma_update(None, "trusted_schema", false)
        .map_err(failed)?;
    let extra: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type IN ('trigger', 'view')",
            [],
            |row| row.get(0),
        )
        .map_err(failed)?;
    if extra > 0 {
        return Err(FOREIGN.into());
    }
    Ok(db)
}

pub fn check_password(password: &str) -> Result<(), String> {
    if password.chars().count() < MIN_PASSWORD {
        return Err(format!(
            "Choose a password of at least {MIN_PASSWORD} characters."
        ));
    }
    Ok(())
}

fn store(error: Error) -> String {
    format!("The password store of this computer could not be used: {error}")
}

pub fn remembered(service: &str, id: &str) -> Option<String> {
    Entry::new(service, id).ok()?.get_password().ok()
}

pub fn remember(service: &str, id: &str, password: Option<&str>) -> Result<(), String> {
    let entry = Entry::new(service, id).map_err(store)?;
    match password {
        Some(password) => entry.set_password(password).map_err(store),
        None => match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(store(error)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(test: &str, schema: &str) -> std::path::PathBuf {
        let file =
            std::env::temp_dir().join(format!("znote-sealed-{test}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let db = Connection::open(&file).unwrap();
        db.pragma_update(None, "key", "a fine password").unwrap();
        db.execute_batch(schema).unwrap();
        file
    }

    #[test]
    fn a_wrong_password_says_so_and_a_view_is_refused_like_a_trigger() {
        let plain = made("plain", "CREATE TABLE notes (text TEXT);");
        assert!(open(&plain, "a fine password", OpenFlags::default(), "wrong").is_ok());
        assert_eq!(
            open(&plain, "another password", OpenFlags::default(), "wrong").unwrap_err(),
            "wrong"
        );
        let viewed = made(
            "view",
            "CREATE TABLE notes (text TEXT); CREATE VIEW seen AS SELECT text FROM notes;",
        );
        assert_eq!(
            open(&viewed, "a fine password", OpenFlags::default(), "wrong").unwrap_err(),
            FOREIGN
        );
    }

    #[test]
    fn a_password_is_counted_in_characters_not_bytes() {
        assert!(check_password("ééééééé").is_err());
        assert!(check_password("éééééééé").is_ok());
        assert!(check_password("").is_err());
    }
}
