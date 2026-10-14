mod agent;
mod compile;
mod export;
mod extract;
mod knowledge;
mod models;
mod privacy;
mod project;
mod snapshot;
mod theme;
mod workspace;

use std::path::Path;

use tauri::{Emitter, Manager};

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
        .manage(agent::Agent::default())
        .manage(project::Project::default())
        .manage(knowledge::Knowledge::default())
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
            project::save_snapshot,
            project::project_asked,
            project::project_open,
            project::project_new,
            project::project_note,
            export::export_page,
            export::open_export,
            export::show_export,
            agent::agent_status,
            agent::agent_install,
            agent::agent_connect,
            agent::agent_code,
            agent::agent_disconnect,
            agent::agent_cancel,
            agent::agent_run,
            agent::agent_attach,
            agent::agent_level,
            agent::agent_forget,
            agent::provider_connect,
            agent::provider_models,
            agent::chats_list,
            agent::chat_save,
            knowledge::privacy_options,
            knowledge::privacy_pages,
            knowledge::privacy_view,
            knowledge::privacy_override,
            knowledge::privacy_overrides,
            knowledge::privacy_forget,
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
                app.state::<agent::Agent>().cancel_all();
                project::save(app);
            }
        });
}
