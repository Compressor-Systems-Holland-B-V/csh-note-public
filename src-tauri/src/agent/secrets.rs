use keyring::{Entry, Error};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "znote assistant";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Secret {
    pub url: String,
    pub key: String,
}

fn locked(error: Error) -> String {
    format!("The password store of this computer could not be used: {error}")
}

fn entry(provider: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, provider).map_err(locked)
}

pub fn read(provider: &str) -> Result<Option<Secret>, String> {
    match entry(provider)?.get_password() {
        Ok(stored) => Ok(serde_json::from_str(&stored).ok()),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(locked(error)),
    }
}

pub fn write(provider: &str, secret: &Secret) -> Result<(), String> {
    let stored = serde_json::to_string(secret).map_err(|error| error.to_string())?;
    entry(provider)?.set_password(&stored).map_err(locked)
}

pub fn clear(provider: &str) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(locked(error)),
    }
}
