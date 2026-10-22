use crate::Result;
use crate::hyprland::{Hyprland, Window};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub fn wait_for_destination(hyprland: &Hyprland, timeout: Duration) -> Result<Window> {
    let deadline = Instant::now() + timeout;
    if hyprland.active()?.is_picker() {
        hyprland.dispatch("focuswindow previous")?;
    }
    loop {
        if let Ok(window) = hyprland.active()
            && !window.class().is_empty()
            && !window.is_picker()
        {
            return Ok(window);
        }
        if Instant::now() >= deadline {
            return Err(
                "The picker still has focus. The text is copied; paste it manually.".into(),
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn keys(ghostty: bool) -> Vec<&'static str> {
    if ghostty {
        vec![
            "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
        ]
    } else {
        vec!["-M", "ctrl", "-k", "v", "-m", "ctrl"]
    }
}

pub fn after_picker() -> Result<()> {
    thread::sleep(Duration::from_millis(150));
    let hyprland = Hyprland::discover()?;
    let destination = wait_for_destination(&hyprland, Duration::from_millis(700))?;
    let active = hyprland.active()?;
    if active.address != destination.address || active.is_picker() || active.class().is_empty() {
        return Err("Focus changed before pasting. The text is copied; paste it manually.".into());
    }
    if !Command::new("wtype")
        .args(keys(active.is_ghostty()))
        .status()?
        .success()
    {
        return Err("Automatic paste failed. The text is copied; paste it manually.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;

    fn fixture(
        replies: Vec<&'static str>,
    ) -> Result<(tempfile::TempDir, Hyprland, thread::JoinHandle<()>)> {
        let temp = tempfile::tempdir()?;
        let socket = temp.path().join("focus.sock");
        let listener = UnixListener::bind(&socket)?;
        let server = thread::spawn(move || {
            for reply in replies {
                let (mut client, _) = listener.accept().unwrap();
                let mut request = String::new();
                client.read_to_string(&mut request).unwrap();
                if reply == "ok" {
                    assert_eq!(request, "dispatch focuswindow previous");
                }
                client.write_all(reply.as_bytes()).unwrap();
            }
        });
        Ok((temp, Hyprland { socket }, server))
    }

    #[test]
    fn waits_until_focus_leaves_both_picker_classes() -> Result<()> {
        let (_temp, hyprland, server) = fixture(vec![
            r#"{"class":"global.snip"}"#,
            "ok",
            r#"{"class":"macro.picker"}"#,
            r#"{"address":"0xabc","class":"editor"}"#,
        ])?;
        assert_eq!(
            wait_for_destination(&hyprland, Duration::from_secs(1))?.address,
            "0xabc"
        );
        server.join().unwrap();
        Ok(())
    }

    #[test]
    fn timeout_never_returns_a_picker_as_a_paste_destination() -> Result<()> {
        let (_temp, hyprland, server) = fixture(vec![
            r#"{"class":"global.snip"}"#,
            "ok",
            r#"{"class":"global.snip"}"#,
        ])?;
        assert!(wait_for_destination(&hyprland, Duration::ZERO).is_err());
        server.join().unwrap();
        Ok(())
    }

    #[test]
    fn terminal_and_normal_app_paste_shortcuts_are_distinct() {
        assert_eq!(keys(false), ["-M", "ctrl", "-k", "v", "-m", "ctrl"]);
        assert_eq!(
            keys(true),
            [
                "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl"
            ]
        );
    }
}
