use std::process::Command;

#[test]
fn weather_cli_works_without_a_display_or_real_credentials() {
    let home = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_desktop-bar");
    let output = Command::new(binary)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join("config"))
        .env("XDG_CACHE_HOME", home.path().join("cache"))
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .arg("--weather")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("Weather unavailable:"));
    assert!(text.contains("API key missing"));
    assert!(!text.contains("<span"));
    let invalid = Command::new(binary)
        .args(["--weather", "--invalid"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
}
