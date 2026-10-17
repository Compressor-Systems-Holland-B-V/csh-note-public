mod agent;
mod compile;
mod export;
mod knowledge;
mod models;
mod policy;
mod privacy;
mod project;
mod snapshot;
mod theme;
mod workspace;

use std::path::Path;

use tauri::{Emitter, Manager};

pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| "This could not be finished. Try again.".to_string())?
}

pub fn keep(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temp = file.to_path_buf();
    temp.as_mut_os_string().push(".tmp");
    std::fs::File::create(&temp).and_then(|mut handle| {
        std::io::Write::write_all(&mut handle, bytes)?;
        handle.sync_all()
    })?;
    std::fs::rename(&temp, file)
}

pub fn guarded<T>(failure: &str, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err(failure.to_string()))
}

#[tauri::command]
fn app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, arguments, folder| {
                if let Some(file) = project::asked(&arguments, Path::new(&folder)) {
                    let _ = app.emit("project-asked", file);
                }
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            },
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(compile::Compiler::default())
        .manage(workspace::Workspace::default())
        .manage(export::Exports::default())
        .manage(project::Project::default())
        .plugin(tauri_plugin_zsuite_assistant::init(
            agent::Assistant,
            agent::KEYS,
            zsuite_sealed::Knowledge::new(policy::get().clone())
                .with_glossary(privacy::default_glossary()),
        ))
        .manage(knowledge::Pending::default())
        .invoke_handler(tauri::generate_handler![
            app_version,
            compile::compile,
            workspace::pick_typst_file,
            workspace::open_workspace,
            workspace::list_tree,
            workspace::create_entry,
            workspace::move_entry,
            workspace::delete_entry,
            workspace::read_page,
            workspace::search_pages,
            workspace::write_page,
            theme::list_themes,
            theme::import_theme,
            theme::store_themes,
            theme::install_store_theme,
            theme::remove_theme,
            project::project_save,
            project::save_snapshot,
            project::project_asked,
            project::project_open,
            project::project_new,
            project::project_unlock,
            project::project_note,
            export::export_page,
            export::open_export,
            export::show_export,
            knowledge::knowledge_status,
            knowledge::knowledge_create,
            knowledge::knowledge_pick,
            knowledge::knowledge_import,
            knowledge::knowledge_unlock,
            knowledge::knowledge_activate,
            knowledge::knowledge_export,
            knowledge::knowledge_password,
            knowledge::knowledge_remove,
            knowledge::knowledge_sync,
            knowledge::knowledge_add,
            knowledge::knowledge_attach,
            knowledge::knowledge_forget,
            knowledge::knowledge_rebuild,
            knowledge::glossary_import,
            models::models_download,
            models::models_cancel,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(false) = event {
                let app = window.app_handle().clone();
                std::thread::spawn(move || project::save(&app));
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<zsuite_agent::Agent>().cancel_all();
                project::close(app);
            }
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_panic_in_guarded_work_becomes_a_message() {
        let failed: Result<(), String> = super::guarded("It broke.", || panic!("bad input"));
        assert_eq!(failed, Err("It broke.".into()));
        assert_eq!(super::guarded("It broke.", || Ok(7)), Ok(7));
    }
}
