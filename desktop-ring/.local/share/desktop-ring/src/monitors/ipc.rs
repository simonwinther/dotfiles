use super::{Monitor, Rule, needs_apply, validate_plan};
use crate::core::Event;
use anyhow::{Context, Result, ensure};
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc,
};

pub fn socket_dir() -> Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR missing")?;
    let root = PathBuf::from(runtime).join("hypr");
    resolve_socket_dir(
        &root,
        std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok().as_deref(),
    )
}
fn resolve_socket_dir(root: &std::path::Path, signature: Option<&str>) -> Result<PathBuf> {
    if let Some(sig) = signature {
        let dir = root.join(sig);
        if std::os::unix::net::UnixStream::connect(dir.join(".socket.sock")).is_ok() {
            return Ok(dir);
        }
    }
    let dirs: Vec<_> = std::fs::read_dir(root)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| std::os::unix::net::UnixStream::connect(p.join(".socket.sock")).is_ok())
        .collect();
    ensure!(
        dirs.len() == 1,
        "expected one live Hyprland instance, found {}",
        dirs.len()
    );
    Ok(dirs[0].clone())
}
pub async fn request_at(dir: &std::path::Path, command: &str) -> Result<String> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut s = UnixStream::connect(dir.join(".socket.sock")).await?;
        s.write_all(command.as_bytes()).await?;
        let mut response = String::new();
        s.take(4 * 1024 * 1024)
            .read_to_string(&mut response)
            .await?;
        Ok(response)
    })
    .await
    .context("Hyprland IPC request timed out")?
}
pub async fn discover() -> Result<Vec<Monitor>> {
    let dir = socket_dir()?;
    let mut ms: Vec<Monitor> = serde_json::from_str(&request_at(&dir, "j/monitors all").await?)?;
    for m in &mut ms {
        m.enrich();
    }
    Ok(ms)
}
pub fn lua_rule(r: &Rule) -> Result<String> {
    // Only the connector discovered in this session is used to address IPC; it is never a physical identity.
    ensure!(
        !r.output.is_empty()
            && r.output
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c)),
        "unsafe connector name"
    );
    Ok(format!(
        "hl.monitor({{output={},mode={},position={},scale={},transform={},disabled=false}})",
        serde_json::to_string(&r.output)?,
        serde_json::to_string(&r.mode)?,
        serde_json::to_string(&format!("{}x{}", r.x, r.y))?,
        r.scale,
        r.transform
    ))
}
pub async fn apply(rules: &[Rule], ms: &[Monitor]) -> Result<usize> {
    validate_plan(rules, ms)?;
    let changes: Vec<_> = rules
        .iter()
        .filter(|r| {
            ms.iter()
                .find(|m| m.name == r.output)
                .is_some_and(|m| needs_apply(r, m))
        })
        .collect();
    if changes.is_empty() {
        return Ok(0);
    }
    let dir = socket_dir()?;
    for rule in &changes {
        let reply = request_at(&dir, &format!("/eval {}", lua_rule(rule)?)).await?;
        if reply.trim() != "ok" {
            // Restore the pre-apply modes on any explicit compositor rejection. Never issue a disable.
            for old in ms.iter().filter(|m| !m.disabled) {
                let _ = request_at(
                    &dir,
                    &format!("/eval {}", lua_rule(&super::current_rule(old))?),
                )
                .await;
            }
            anyhow::bail!("Hyprland rejected {}: {}", rule.output, reply.trim());
        }
    }
    eprintln!("displays: applied {} changed outputs", changes.len());
    Ok(changes.len())
}
fn event_name(line: &str) -> &str {
    line.split_once(">>").map_or(line, |(name, _)| name)
}
// Hyprland posts each hotplug as a v1 event directly followed by its v2 twin.
// Skip only that twin, so a lone event of either generation still reconciles.
fn display_event(line: &str, previous: &str) -> bool {
    match event_name(line) {
        "configreloaded" | "monitoradded" | "monitorremoved" => true,
        "monitoraddedv2" => previous != "monitoradded",
        "monitorremovedv2" => previous != "monitorremoved",
        _ => false,
    }
}
pub async fn watch(tx: mpsc::Sender<Event>) {
    let mut backoff = 1;
    loop {
        let result: Result<()> = async {
            let dir = socket_dir()?;
            let stream = UnixStream::connect(dir.join(".socket2.sock")).await?;
            tx.send(Event::Displays).await?;
            let mut lines = BufReader::new(stream).lines();
            let mut previous = String::new();
            while let Some(line) = lines.next_line().await? {
                if display_event(&line, &previous) {
                    tx.send(Event::Displays).await?;
                }
                previous.clear();
                previous.push_str(event_name(&line));
            }
            anyhow::bail!("Hyprland event socket closed")
        }
        .await;
        if tx.is_closed() {
            return;
        }
        eprintln!("displays: {result:?}; reconnect in {backoff}s");
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crashed_compositor_socket_does_not_hide_the_new_instance() {
        use std::os::unix::net::UnixListener;
        let root = std::env::temp_dir().join(format!(
            "ring-ipc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for name in ["crashed", "new", "other"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        let stale = UnixListener::bind(root.join("crashed/.socket.sock")).unwrap();
        drop(stale);
        let active = UnixListener::bind(root.join("new/.socket.sock")).unwrap();
        assert_eq!(
            resolve_socket_dir(&root, Some("crashed")).unwrap(),
            root.join("new")
        );
        let other = UnixListener::bind(root.join("other/.socket.sock")).unwrap();
        assert!(resolve_socket_dir(&root, None).is_err());
        assert_eq!(
            resolve_socket_dir(&root, Some("new")).unwrap(),
            root.join("new")
        );
        drop(active);
        drop(other);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn each_hotplug_reconciles_once_although_both_event_generations_arrive() {
        let triggers = |lines: &[&str]| -> Vec<bool> {
            let mut previous = String::new();
            lines
                .iter()
                .map(|line| {
                    let display = display_event(line, &previous);
                    previous = event_name(line).into();
                    display
                })
                .collect()
        };
        assert_eq!(
            triggers(&["monitoradded>>DP-1", "monitoraddedv2>>1,DP-1,Model"]),
            [true, false]
        );
        assert_eq!(triggers(&["monitoraddedv2>>1,DP-1,Model"]), [true]);
        assert_eq!(
            triggers(&["activewindow>>x", "monitorremovedv2>>1,DP-1,Model"]),
            [false, true]
        );
        assert_eq!(triggers(&["configreloaded>>"]), [true]);
        assert_eq!(
            triggers(&["activewindow>>x", "activewindowv2>>x"]),
            [false, false]
        );
        assert_eq!(
            triggers(&[
                "monitorremoved>>DP-1",
                "monitorremovedv2>>1,DP-1,Model",
                "monitorremoved>>DP-2",
                "monitorremovedv2>>2,DP-2,Model",
            ]),
            [true, false, true, false]
        );
    }
}
