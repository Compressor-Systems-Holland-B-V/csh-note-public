use std::fs;
use std::path::Path;

fn languages() {
    let folder = Path::new("resources/languages");
    println!("cargo:rerun-if-changed={}", folder.display());
    let mut files: Vec<_> = fs::read_dir(folder)
        .expect("resources/languages is readable")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
                && !path.to_string_lossy().ends_with(".schema.json")
        })
        .collect();
    files.sort();
    let bundled: Vec<String> = files
        .iter()
        .map(|file| fs::read_to_string(file).expect("a language file is readable"))
        .collect();
    let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    fs::write(
        Path::new(&out).join("languages.json"),
        format!("[{}]", bundled.join(",")),
    )
    .expect("OUT_DIR is writable");
}

fn main() {
    languages();
    tauri_build::build()
}
