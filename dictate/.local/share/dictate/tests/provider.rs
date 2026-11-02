use std::{fs, path::PathBuf, process::Command};

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dictate-provider-{}", std::process::id()));
        fs::create_dir_all(root.join("config/dictate")).unwrap();
        fs::write(
            root.join("config/dictate/config.env"),
            "DICTIONARY_FILE=''\n",
        )
        .unwrap();
        Self(root)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dictate"));
        command
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_RUNTIME_DIR", self.0.join("run"))
            .env("OPENAI_API_KEY", "test-existing-key")
            .env("SONIOX_API_KEY", "test-soniox-key");
        command
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn switch_persists_for_shortcut_and_missing_key_keeps_previous_choice() {
    let sandbox = Sandbox::new();
    let output = sandbox
        .command()
        .args(["provider", "soniox"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = sandbox.command().arg("provider").output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("soniox (stt-rt-v5)"));
    let file = sandbox.0.join("state/dictate/live-provider");
    assert_eq!(fs::read_to_string(&file).unwrap(), "soniox");
    let output = sandbox
        .command()
        .args(["provider", "openai"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "openai");
    let output = sandbox
        .command()
        .env("SONIOX_API_KEY", "")
        .args(["provider", "soniox"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SONIOX_API_KEY is not configured"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "openai");
    assert!(!fs::read_to_string(&file).unwrap().contains("test-"));
}
