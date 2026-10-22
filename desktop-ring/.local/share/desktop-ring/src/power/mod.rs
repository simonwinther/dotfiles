use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::process::Command;
use zbus::{Connection, Proxy};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PowerState {
    pub available: bool,
    pub can_suspend: bool,
    pub error: Option<String>,
}

pub type Inhibitor = (String, String, String, String, u32, u32);

async fn logind(connection: &Connection) -> Result<Proxy<'_>> {
    Ok(Proxy::new(
        connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?)
}

pub async fn discover() -> PowerState {
    let result: Result<String> = tokio::time::timeout(Duration::from_secs(3), async {
        let connection = Connection::system().await?;
        Ok(logind(&connection).await?.call("CanSuspend", &()).await?)
    })
    .await
    .context("Sleep capability query timed out")
    .and_then(|r| r);
    match result {
        Ok(capability) => PowerState {
            available: true,
            can_suspend: matches!(capability.as_str(), "yes" | "challenge"),
            error: (!matches!(capability.as_str(), "yes" | "challenge"))
                .then(|| "Sleep is unavailable for this session".into()),
        },
        Err(e) => PowerState {
            error: Some(format!("Sleep: {e}")),
            ..Default::default()
        },
    }
}

pub fn has_sleep_guard(inhibitors: &[Inhibitor], pid: u32) -> bool {
    pid != 0
        && inhibitors.iter().any(|(what, _, _, mode, _, owner)| {
            *owner == pid && mode == "delay" && what.split(':').any(|s| s == "sleep")
        })
}

pub async fn prepare() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(8), async {
        let connection = Connection::system().await?;
        check_on(&connection, std::ffi::OsStr::new("systemctl")).await
    })
    .await
    .context("Sleep protection check timed out")?
}

pub async fn suspend() -> Result<()> {
    // Authentication may require user input. The daemon runs this off its event
    // loop after acknowledging the selection so the exclusive menu can close.
    tokio::time::timeout(Duration::from_secs(120), async {
        let connection = Connection::system().await?;
        suspend_on(&connection, std::ffi::OsStr::new("systemctl")).await
    })
    .await
    .context("Sleep authorization timed out")?
}

async fn check_on(connection: &Connection, systemctl: &std::ffi::OsStr) -> Result<()> {
    let proxy = logind(connection).await?;
    let capability: String = proxy.call("CanSuspend", &()).await?;
    ensure!(
        matches!(capability.as_str(), "yes" | "challenge"),
        "Sleep is unavailable"
    );
    // Require the existing session daemon's actual logind inhibitor, not merely
    // a running locker process. Hypridle releases it on Wayland lock notification.
    let output = Command::new(systemctl)
        .kill_on_drop(true)
        .args([
            "--user",
            "show",
            "--property=MainPID",
            "--value",
            "desktop-hypridle.service",
        ])
        .output()
        .await
        .context("Cannot inspect sleep protection")?;
    ensure!(output.status.success(), "Cannot inspect sleep protection");
    let pid = String::from_utf8(output.stdout)?
        .trim()
        .parse()
        .unwrap_or(0);
    let inhibitors: Vec<Inhibitor> = proxy.call("ListInhibitors", &()).await?;
    ensure!(
        has_sleep_guard(&inhibitors, pid),
        "Sleep protection is not ready; check desktop-hypridle.service before retrying"
    );
    Ok(())
}

async fn suspend_on(connection: &Connection, systemctl: &std::ffi::OsStr) -> Result<()> {
    check_on(connection, systemctl).await?;
    let output = Command::new(systemctl)
        .kill_on_drop(true)
        .args(["--no-block", "suspend"])
        .output()
        .await
        .context("Cannot request sleep")?;
    ensure!(
        output.status.success(),
        "Sleep failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        os::unix::fs::PermissionsExt,
        process::{Child, Stdio},
        sync::{Arc, Mutex},
    };

    struct Bus(Child, std::path::PathBuf);
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }
    struct Login(Arc<Mutex<(String, Vec<Inhibitor>)>>);
    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Login {
        fn can_suspend(&self) -> String {
            self.0.lock().unwrap().0.clone()
        }
        fn list_inhibitors(&self) -> Vec<Inhibitor> {
            self.0.lock().unwrap().1.clone()
        }
    }

    #[tokio::test]
    async fn dbus_guard_and_capability_gate_the_real_command_path() {
        tokio::time::timeout(Duration::from_secs(10), exercise_private_bus())
            .await
            .expect("private logind fixture timed out");
    }

    async fn exercise_private_bus() {
        let folder = std::env::temp_dir().join(format!("ring-power-{}", std::process::id()));
        std::fs::create_dir(&folder).unwrap();
        let config = folder.join("bus.conf");
        std::fs::write(&config, r#"<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>"#).unwrap();
        let mut child = std::process::Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let _bus = Bus(child, folder.clone());
        let state = Arc::new(Mutex::new(("yes".into(), vec![])));
        let _server = zbus::connection::Builder::address(address.trim())
            .unwrap()
            .name("org.freedesktop.login1")
            .unwrap()
            .serve_at("/org/freedesktop/login1", Login(state.clone()))
            .unwrap()
            .build()
            .await
            .unwrap();
        let client = zbus::connection::Builder::address(address.trim())
            .unwrap()
            .build()
            .await
            .unwrap();
        let binary = folder.join("systemctl");
        let marker = folder.join("suspend-requested");
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        std::fs::write(&binary, format!("#!{shell}\ncase \"$*\" in\n'--user show --property=MainPID --value desktop-hypridle.service') echo 42;;\n'--no-block suspend') echo requested > '{}' ;;\n*) exit 9;;\nesac\n", marker.display())).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            suspend_on(&client, binary.as_os_str())
                .await
                .unwrap_err()
                .to_string()
                .contains("Sleep protection")
        );
        assert!(!marker.exists());
        state.lock().unwrap().1 = vec![(
            "sleep".into(),
            "hypridle".into(),
            "locking".into(),
            "delay".into(),
            1001,
            42,
        )];
        state.lock().unwrap().0 = "no".into();
        assert!(
            suspend_on(&client, binary.as_os_str())
                .await
                .unwrap_err()
                .to_string()
                .contains("unavailable")
        );
        assert!(!marker.exists());
        state.lock().unwrap().0 = "challenge".into();
        suspend_on(&client, binary.as_os_str()).await.unwrap();
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "requested\n");
        std::fs::write(
            &binary,
            format!(
                "#!{shell}\ncase \"$1\" in --user) echo 42;; *) echo denied >&2; exit 1;; esac\n"
            ),
        )
        .unwrap();
        assert!(
            suspend_on(&client, binary.as_os_str())
                .await
                .unwrap_err()
                .to_string()
                .contains("denied")
        );
    }
    #[test]
    fn sleep_requires_the_session_daemons_delay_inhibitor() {
        let guard = (
            "sleep:idle".into(),
            "hypridle".into(),
            "Lock before sleep".into(),
            "delay".into(),
            1001,
            42,
        );
        assert!(has_sleep_guard(std::slice::from_ref(&guard), 42));
        assert!(!has_sleep_guard(std::slice::from_ref(&guard), 43));
        assert!(!has_sleep_guard(std::slice::from_ref(&guard), 0));
        assert!(!has_sleep_guard(&[], 42));
        for (what, mode) in [("idle", "delay"), ("sleep", "block")] {
            let mut other = guard.clone();
            other.0 = what.into();
            other.3 = mode.into();
            assert!(!has_sleep_guard(&[other], 42));
        }
    }
}
