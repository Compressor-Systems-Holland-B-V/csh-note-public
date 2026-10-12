#[test]
fn package_json_and_cargo_toml_have_the_same_version() {
    let expected = format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(include_str!("../package.json").contains(&expected));
}
