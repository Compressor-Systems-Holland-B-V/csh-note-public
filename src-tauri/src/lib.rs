mod compile;

#[tauri::command]
fn app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(compile::Compiler::default())
        .invoke_handler(tauri::generate_handler![app_version, compile::compile])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
