use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use zsuite_finder::models::{Model, Piece, Progress};

use crate::knowledge;

static STOP_NAMES: AtomicBool = AtomicBool::new(false);
static STOP_SEARCH: AtomicBool = AtomicBool::new(false);

fn stop(id: &str) -> &'static AtomicBool {
    if id == "names" {
        &STOP_NAMES
    } else {
        &STOP_SEARCH
    }
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

pub fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("models"))
}

fn web(address: &str) -> Result<Box<dyn Read>, String> {
    let response = zsuite_agent::connection()?
        .build()
        .get(address)
        .call()
        .map_err(zsuite_agent::refused)?;
    Ok(Box::new(response.into_reader()))
}

#[tauri::command]
pub async fn models_download(
    app: AppHandle,
    model: String,
    progress: Channel<Progress>,
) -> Result<(), String> {
    crate::blocking(move || models_download_now(app, model, progress)).await
}

fn models_download_now(
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
    chosen.fetch(&folder(&app)?, &web, stop(chosen.id), &|step| {
        let _ = progress.send(step);
    })?;
    knowledge::models_ready(&app, chosen.id);
    Ok(())
}

#[tauri::command]
pub fn models_cancel(model: String) {
    if matches!(model.as_str(), "names" | "search") {
        stop(&model).store(true, Ordering::Relaxed);
    }
}
