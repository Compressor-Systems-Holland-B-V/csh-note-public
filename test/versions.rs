#[test]
fn package_json_and_cargo_toml_have_the_same_version() {
    let expected = format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(include_str!("../package.json").contains(&expected));
    assert!(include_str!("../src-tauri/tauri.conf.json").contains(&expected));
}

#[test]
fn the_newest_release_in_the_linux_metainfo_is_this_version() {
    let version = env!("CARGO_PKG_VERSION");
    let metainfo = include_str!("../packaging/linux/com.zalanalytics.znote.metainfo.xml");
    let newest = metainfo.split("<release ").nth(1).unwrap();
    assert!(newest.starts_with(&format!("version=\"{version}\"")));
}
