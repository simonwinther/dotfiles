use crate::Result;
use crate::hyprland::{Hyprland, Window};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub fn wait_for_destination(hyprland: &Hyprland, timeout: Duration) -> Result<Window> {
    let deadline = Instant::now() + timeout;
    let mut window = hyprland.active()?;
    if window.is_picker() {
        hyprland.dispatch("hl.dsp.focus({ last = true })")?;
        window = hyprland.active()?;
    }
    loop {
        if !window.class().is_empty() && !window.is_picker() {
            return Ok(window);
        }
        if Instant::now() >= deadline {
            return Err(
                "The picker still has focus. The text is copied; paste it manually.".into(),
            );
        }
        thread::sleep(Duration::from_millis(20));
        window = hyprland.active().unwrap_or_default();
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
    paste_with(&Hyprland::discover()?, &mut Command::new("wtype"))
}

fn paste_with(hyprland: &Hyprland, command: &mut Command) -> Result<()> {
    let destination = wait_for_destination(hyprland, Duration::from_millis(700))?;
    let active = hyprland.active()?;
    if active.address != destination.address || active.is_picker() || active.class().is_empty() {
        return Err("Focus changed before pasting. The text is copied; paste it manually.".into());
    }
    if !command.args(keys(active.is_ghostty())).status()?.success() {
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
                    assert_eq!(request, "dispatch hl.dsp.focus({ last = true })");
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

    #[test]
    fn focus_changes_block_pasting() -> Result<()> {
        let (_temp, hyprland, server) = fixture(vec![
            r#"{"address":"0xabc","class":"editor"}"#,
            r#"{"address":"0xdef","class":"editor"}"#,
        ])?;
        let error = paste_with(&hyprland, &mut Command::new("does-not-exist"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Focus changed"));
        server.join().unwrap();
        Ok(())
    }

    #[test]
    fn paste_command_failures_leave_manual_paste_available() -> Result<()> {
        let (_temp, hyprland, server) = fixture(vec![
            r#"{"address":"0xabc","class":"editor"}"#,
            r#"{"address":"0xabc","class":"editor"}"#,
        ])?;
        let error = paste_with(&hyprland, &mut Command::new("false"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Automatic paste failed"));
        server.join().unwrap();
        Ok(())
    }

    #[test]
    #[ignore = "manual latency measurement with fake Hyprland IPC and no keyboard input"]
    fn benchmark_ready_paste() -> Result<()> {
        let mut samples = Vec::new();
        for _ in 0..20 {
            let (_temp, hyprland, server) = fixture(vec![
                r#"{"address":"0xabc","class":"editor"}"#,
                r#"{"address":"0xabc","class":"editor"}"#,
            ])?;
            let started = Instant::now();
            paste_with(&hyprland, &mut Command::new("true"))?;
            samples.push(started.elapsed().as_secs_f64() * 1000.0);
            server.join().unwrap();
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "Ready paste, fake IPC + true: median={:.2} ms, max={:.2} ms",
            samples[10], samples[19]
        );
        Ok(())
    }
}
