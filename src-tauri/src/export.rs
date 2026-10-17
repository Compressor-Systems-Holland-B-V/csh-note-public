use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use zsuite_typst::export::{docx, math, pdf, pptx, prelude};

use crate::compile;

const BROKE: &str = "This page could not be exported. Simplify it, for example make the page smaller, and try again.";

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Pdf,
    Pptx,
    Docx,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    path: String,
}

#[derive(Default)]
pub struct Exports {
    last: Mutex<Option<PathBuf>>,
}

impl Exports {
    fn last(&self) -> Result<PathBuf, String> {
        let last = self.last.lock().map_err(|error| error.to_string())?;
        last.clone()
            .ok_or_else(|| "Nothing has been exported yet.".to_string())
    }
}

#[tauri::command]
pub async fn export_page(
    app: AppHandle,
    page: String,
    text: String,
    format: Format,
) -> Result<Option<Exported>, String> {
    crate::blocking(move || export_now(&app, &page, text, format)).await
}

fn export_now(
    app: &AppHandle,
    page: &str,
    text: String,
    format: Format,
) -> Result<Option<Exported>, String> {
    let exports = app.state::<Exports>();
    let page = page.to_string();
    let (document, html) = compile::with_page(app, &page, |world| {
        let document = world.document(text.clone())?;
        let html =
            match format {
                Format::Pdf => None,
                // Without the HTML compile the formulas stay in the slide picture.
                Format::Pptx => world.html(text, &prelude(&document)).ok(),
                Format::Docx => Some(world.html(text, &prelude(&document)).map_err(|error| {
                    format!("This page cannot become a Word document: {error}")
                })?),
            };
        Ok((document, html))
    })?;
    let (label, extension) = match format {
        Format::Pdf => ("PDF", "pdf"),
        Format::Pptx => ("PowerPoint", "pptx"),
        Format::Docx => ("Word", "docx"),
    };
    let name = page.rsplit('/').next().unwrap_or(&page);
    let dialog = app
        .dialog()
        .file()
        .set_file_name(format!("{name}.{extension}"))
        .add_filter(label, &[extension]);
    let Some(target) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let mut path = target.into_path().map_err(|error| error.to_string())?;
    if path
        .extension()
        .is_none_or(|given| !given.eq_ignore_ascii_case(extension))
    {
        path.as_mut_os_string().push(format!(".{extension}"));
    }

    let bytes = match (format, html) {
        (Format::Pdf, _) => crate::guarded(BROKE, || pdf(&document))?,
        (Format::Pptx, html) => {
            let equations = html
                .map(|(html, _)| math::equations(&html))
                .unwrap_or_default();
            crate::guarded(BROKE, || pptx::pptx(&document, &equations))?
        }
        (Format::Docx, Some((html, _))) => {
            let equations = math::equations(&html);
            crate::guarded(BROKE, || docx::docx(&document, &html, &equations))?
        }
        (Format::Docx, None) => return Err(BROKE.into()),
    };
    std::fs::write(&path, bytes)
        .map_err(|error| format!("The file could not be saved: {error}"))?;
    *exports.last.lock().map_err(|error| error.to_string())? = Some(path.clone());
    Ok(Some(Exported {
        path: path.to_string_lossy().into_owned(),
    }))
}

#[tauri::command]
pub fn open_export(app: AppHandle, exports: State<Exports>) -> Result<(), String> {
    let path = exports.last()?.to_string_lossy().into_owned();
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|error| format!("The file could not be opened: {error}"))
}

#[tauri::command]
pub fn show_export(app: AppHandle, exports: State<Exports>) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(exports.last()?)
        .map_err(|error| format!("The folder could not be opened: {error}"))
}
