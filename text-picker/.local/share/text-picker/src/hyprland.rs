use crate::Result;
use serde::Deserialize;
use std::env;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone)]
pub struct Hyprland {
    pub socket: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Window {
    pub address: String,
    pub class: String,
    #[serde(rename = "initialClass")]
    pub initial_class: String,
}

impl Window {
    pub fn class(&self) -> &str {
        if self.class.is_empty() {
            &self.initial_class
        } else {
            &self.class
        }
    }

    pub fn is_picker(&self) -> bool {
        matches!(self.class(), "global.snip" | "macro.picker")
    }

    pub fn is_ghostty(&self) -> bool {
        matches!(self.class(), "ghostty" | "com.mitchellh.ghostty")
            || self.class().contains("Ghostty")
    }
}

impl Hyprland {
    pub fn discover() -> Result<Self> {
        let runtime =
            env::var_os("XDG_RUNTIME_DIR").ok_or("A running Hyprland session is required")?;
        let signature = env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
            .ok_or("A running Hyprland session is required")?;
        if signature.is_empty() || signature.as_encoded_bytes().contains(&b'/') {
            return Err("Invalid Hyprland instance signature".into());
        }
        Ok(Self {
            socket: PathBuf::from(runtime)
                .join("hypr")
                .join(signature)
                .join(".socket.sock"),
        })
    }

    pub fn request(&self, message: &str) -> Result<String> {
        let mut socket = UnixStream::connect(&self.socket)?;
        socket.set_read_timeout(Some(Duration::from_millis(250)))?;
        socket.set_write_timeout(Some(Duration::from_millis(250)))?;
        socket.write_all(message.as_bytes())?;
        socket.shutdown(Shutdown::Write)?;
        let mut response = String::new();
        socket.take(1024 * 1024).read_to_string(&mut response)?;
        Ok(response)
    }

    pub fn active(&self) -> Result<Window> {
        Ok(serde_json::from_str(&self.request("j/activewindow")?)?)
    }

    pub fn exec(&self, command: &str) -> Result<()> {
        self.dispatch(&format!("hl.dsp.exec_cmd({})", lua_quote(command)))
    }

    /// Dispatch a Lua dispatcher expression on Hyprland 0.55 and later.
    pub fn dispatch(&self, command: &str) -> Result<()> {
        let response = self.request(&format!("dispatch {command}"))?;
        if response.trim() != "ok" {
            return Err(format!("Hyprland could not {command}: {}", response.trim()).into());
        }
        Ok(())
    }
}

fn lua_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\0'..='\u{1f}' | '\u{7f}' => {
                // Lua uses decimal byte escapes, unlike JSON's \\u escapes.
                write!(quoted, "\\{:03}", character as u32).unwrap();
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    #[test]
    fn command_quoting_preserves_utf8_quotes_and_control_bytes() {
        assert_eq!(
            lua_quote("\"\\\n\r\t\0\u{7f}9æ"),
            "\"\\\"\\\\\\010\\013\\009\\000\\1279æ\""
        );
    }

    #[test]
    fn exec_uses_a_lua_dispatcher_and_reports_rejections() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("hypr.sock");
        let listener = UnixListener::bind(&path)?;
        let server = thread::spawn(move || {
            for reply in ["ok", "error: launch rejected"] {
                let (mut client, _) = listener.accept().unwrap();
                let mut message = String::new();
                client.read_to_string(&mut message).unwrap();
                assert_eq!(
                    message,
                    r#"dispatch hl.dsp.exec_cmd("'/tmp/helper \\\"quoted\\\"' --handoff '/tmp/helper.sock'")"#
                );
                client.write_all(reply.as_bytes()).unwrap();
            }
        });
        let hyprland = Hyprland { socket: path };
        let command = r#"'/tmp/helper \"quoted\"' --handoff '/tmp/helper.sock'"#;
        hyprland.exec(command)?;
        assert!(
            hyprland
                .exec(command)
                .unwrap_err()
                .to_string()
                .contains("launch rejected")
        );
        server.join().unwrap();
        Ok(())
    }

    #[test]
    fn native_ipc_parses_window_json_without_external_commands() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("hypr.sock");
        let listener = UnixListener::bind(&path)?;
        let server = thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut message = String::new();
            client.read_to_string(&mut message).unwrap();
            assert_eq!(message, "j/activewindow");
            client
                .write_all(
                    br#"{"address":"0x123","class":"","initialClass":"com.mitchellh.ghostty"}"#,
                )
                .unwrap();
        });
        let window = Hyprland { socket: path }.active()?;
        assert_eq!(window.address, "0x123");
        assert!(window.is_ghostty());
        assert!(!window.is_picker());
        server.join().unwrap();
        Ok(())
    }
}
