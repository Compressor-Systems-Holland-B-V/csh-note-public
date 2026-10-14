use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::knowledge;

const HOST: &str = "https://huggingface.co";

static STOP_NAMES: AtomicBool = AtomicBool::new(false);
static STOP_SEARCH: AtomicBool = AtomicBool::new(false);

fn stop(id: &str) -> &'static AtomicBool {
    if id == "names" {
        &STOP_NAMES
    } else {
        &STOP_SEARCH
    }
}

type Source<'a> = dyn Fn(&str) -> Result<Box<dyn Read>, String> + 'a;

pub struct Piece {
    remote: &'static str,
    local: &'static str,
    sha256: &'static str,
    size: u64,
}

pub struct Model {
    id: &'static str,
    folder: &'static str,
    repository: &'static str,
    revision: &'static str,
    pieces: &'static [Piece],
}

pub const NAME_FINDER: Model = Model {
    id: "names",
    folder: "name-finder",
    repository: "knowledgator/gliner-pii-edge-v1.0",
    revision: "9b7f39b0a2da971a5beea78d35f1539d4009c891",
    pieces: &[
        Piece {
            remote: "onnx/model_quint8.onnx",
            local: "model.onnx",
            sha256: "988acb03456b26e2d9f2521016d820310c2ed64deb4a846297d3289f0c2eb7e4",
            size: 45_820_894,
        },
        Piece {
            remote: "tokenizer.json",
            local: "tokenizer.json",
            sha256: "84b3a9b18f04a0ccd03b72d9f871b7e0bec40fd7021ef50bc30a7c3693c11205",
            size: 3_583_593,
        },
    ],
};

pub const SMART_SEARCH: Model = Model {
    id: "search",
    folder: "smart-search",
    repository: "minishlab/potion-base-8M",
    revision: "bf8b056651a2c21b8d2565580b8569da283cab23",
    pieces: &[
        Piece {
            remote: "model.safetensors",
            local: "model.safetensors",
            sha256: "f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2",
            size: 30_236_760,
        },
        Piece {
            remote: "tokenizer.json",
            local: "tokenizer.json",
            sha256: "e67e803f624fb4d67dea1c730d06e1067e1b14d830e2c2202569e3ef0f70bb50",
            size: 683_666,
        },
        Piece {
            remote: "config.json",
            local: "config.json",
            sha256: "2a6ac0e9aaa356a68a5688070db78fc3a464fefe85d2f06a1905ce3718687553",
            size: 202,
        },
    ],
};

#[derive(Clone, Serialize)]
pub struct Progress {
    done: u64,
    total: u64,
}

pub fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("models"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Model {
    pub fn path(&self, models: &Path) -> PathBuf {
        models.join(self.folder)
    }

    pub fn present(&self, models: &Path) -> bool {
        let folder = self.path(models);
        self.pieces.iter().all(|piece| {
            fs::metadata(folder.join(piece.local)).is_ok_and(|meta| meta.len() == piece.size)
        })
    }

    fn total(&self) -> u64 {
        self.pieces.iter().map(|piece| piece.size).sum()
    }

    pub fn fetch(
        &self,
        models: &Path,
        read: &Source<'_>,
        progress: &dyn Fn(Progress),
    ) -> Result<(), String> {
        let folder = self.path(models);
        fs::create_dir_all(&folder)
            .map_err(|error| format!("The download could not be saved: {error}"))?;
        let total = self.total();
        let stop = stop(self.id);
        let mut done = 0;
        for piece in self.pieces {
            let target = folder.join(piece.local);
            if fs::metadata(&target).is_ok_and(|meta| meta.len() == piece.size) {
                done += piece.size;
                continue;
            }
            let partial = folder.join(format!("{}.part", piece.local));
            let fetched = (|| {
                let mut source = read(&format!(
                    "{HOST}/{}/resolve/{}/{}",
                    self.repository, self.revision, piece.remote
                ))?;
                let mut file = File::create(&partial).map_err(|error| error.to_string())?;
                let mut digest = Sha256::new();
                let mut buffer = vec![0; 1 << 16];
                let mut size = 0u64;
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return Err("The download was cancelled.".to_string());
                    }
                    let read = source
                        .read(&mut buffer)
                        .map_err(|error| error.to_string())?;
                    if read == 0 {
                        break;
                    }
                    size += read as u64;
                    if size > piece.size {
                        return Err("The download is larger than expected.".into());
                    }
                    digest.update(&buffer[..read]);
                    file.write_all(&buffer[..read])
                        .map_err(|error| error.to_string())?;
                    progress(Progress {
                        done: done + size,
                        total,
                    });
                }
                file.sync_all().map_err(|error| error.to_string())?;
                if hex(&digest.finalize()) != piece.sha256 {
                    return Err("The download did not match what was expected. Try again.".into());
                }
                fs::rename(&partial, &target).map_err(|error| error.to_string())
            })();
            if let Err(error) = fetched {
                let _ = fs::remove_file(&partial);
                return Err(if error.starts_with("The ") {
                    error
                } else {
                    format!("The download failed: {error}")
                });
            }
            done += piece.size;
        }
        Ok(())
    }
}

fn web(address: &str) -> Result<Box<dyn Read>, String> {
    let response = crate::agent::connection()?
        .build()
        .get(address)
        .call()
        .map_err(crate::agent::refused)?;
    Ok(Box::new(response.into_reader()))
}

#[tauri::command(async)]
pub fn models_download(
    app: AppHandle,
    model: String,
    progress: Channel<Progress>,
) -> Result<(), String> {
    let chosen = match model.as_str() {
        "names" => &NAME_FINDER,
        "search" => &SMART_SEARCH,
        _ => return Err(format!("“{model}” is not a model znote knows.")),
    };
    stop(chosen.id).store(false, Ordering::Relaxed);
    chosen.fetch(&folder(&app)?, &web, &|step| {
        let _ = progress.send(step);
    })?;
    knowledge::models_ready(&app, chosen.id);
    Ok(())
}

#[tauri::command]
pub fn models_cancel(model: String) {
    stop(&model).store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex;

    use super::*;

    const TINY: Model = Model {
        id: "tiny",
        folder: "tiny",
        repository: "someone/tiny",
        revision: "abc",
        pieces: &[Piece {
            remote: "nested/weights.bin",
            local: "weights.bin",
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            size: 5,
        }],
    };

    #[test]
    fn a_download_is_kept_only_when_its_checksum_matches() {
        let models = std::env::temp_dir().join(format!("znote-models-{}", std::process::id()));
        let _ = fs::remove_dir_all(&models);
        let asked = Mutex::new(Vec::new());
        let bad = |address: &str| -> Result<Box<dyn Read>, String> {
            asked.lock().unwrap().push(address.to_string());
            Ok(Box::new(Cursor::new(b"hellO".to_vec())))
        };
        assert!(
            TINY.fetch(&models, &bad, &|_| {})
                .unwrap_err()
                .contains("did not match")
        );
        assert!(!TINY.present(&models));
        assert!(!TINY.path(&models).join("weights.bin.part").exists());
        assert_eq!(
            asked.lock().unwrap()[0],
            "https://huggingface.co/someone/tiny/resolve/abc/nested/weights.bin"
        );
        let good = |_: &str| -> Result<Box<dyn Read>, String> {
            Ok(Box::new(Cursor::new(b"hello".to_vec())))
        };
        let seen = Mutex::new(Vec::new());
        TINY.fetch(&models, &good, &|step| seen.lock().unwrap().push(step.done))
            .unwrap();
        assert!(TINY.present(&models));
        assert_eq!(*seen.lock().unwrap(), [5]);
        let long = |_: &str| -> Result<Box<dyn Read>, String> {
            Ok(Box::new(Cursor::new(b"hello world".to_vec())))
        };
        fs::remove_file(TINY.path(&models).join("weights.bin")).unwrap();
        assert!(
            TINY.fetch(&models, &long, &|_| {})
                .unwrap_err()
                .contains("larger")
        );
    }
}
