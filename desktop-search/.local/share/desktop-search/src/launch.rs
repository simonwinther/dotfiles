use crate::{applications::executable, backend::Backend, config::private_directory};
use anyhow::{Context, Result, ensure};
use std::{
    fs::{File, OpenOptions},
    io::{Seek, Write},
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{OnceLock, mpsc},
    time::{Duration, Instant},
};

static CHILDREN: OnceLock<mpsc::Sender<Child>> = OnceLock::new();

pub fn start(arguments: &[String], context: &str) -> Result<()> {
    ensure!(!arguments.is_empty(), "Application command is empty");
    let mut args = arguments.to_vec();
    if !context.is_empty() {
        ensure!(
            context.len() <= 128
                && context
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
            "Invalid launch context"
        );
        args = vec![
            "hyprspace-launch".into(),
            "--context".into(),
            context.into(),
            "--".into(),
        ]
        .into_iter()
        .chain(args)
        .collect();
    }
    if !matches!(
        Path::new(&args[0])
            .file_name()
            .and_then(|name| name.to_str()),
        Some("uwsm-app" | "uwsm" | "app2unit" | "systemd-run")
    ) {
        if executable("uwsm-app").is_some() {
            args = vec!["uwsm-app".into(), "--".into()]
                .into_iter()
                .chain(args)
                .collect();
        } else if executable("systemd-run").is_some() {
            args = vec![
                "systemd-run".into(),
                "--user".into(),
                "--scope".into(),
                "--collect".into(),
                "--quiet".into(),
                "--".into(),
            ]
            .into_iter()
            .chain(args)
            .collect();
        }
    }
    let mut command = Command::new(&args[0]);
    command
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    unsafe {
        command.pre_exec(|| {
            crate::reset_child_signals();
            Ok(())
        });
    }
    let child = command.spawn().context("Start application")?;
    let children = CHILDREN.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Child>();
        std::thread::Builder::new()
            .name("search-applications".into())
            .stack_size(128 * 1024)
            .spawn(move || {
                let mut children = vec![];
                loop {
                    if children.is_empty() {
                        match receiver.recv() {
                            Ok(child) => children.push(child),
                            Err(_) => return,
                        }
                    } else if let Ok(child) = receiver.recv_timeout(Duration::from_secs(1)) {
                        children.push(child);
                    }
                    children
                        .retain_mut(|child| child.try_wait().is_ok_and(|status| status.is_none()));
                }
            })
            .expect("application reaper");
        sender
    });
    children
        .send(child)
        .map_err(|_| anyhow::anyhow!("Application reaper stopped"))?;
    Ok(())
}

pub fn copy_file(path: &Path, mime: &str) -> Result<()> {
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= 256 * 1024 * 1024,
        "Clipboard payload is too large"
    );
    let mut child = Command::new("wl-copy")
        .args(["--type", mime])
        .stdin(file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Could not acquire the clipboard");
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Clipboard acquisition timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn copy_text(backend: &Backend, text: &str, mime: &str) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static IDS: AtomicU64 = AtomicU64::new(0);
    let directory = backend.paths.state.join("outgoing");
    private_directory(&directory)?;
    let path = directory.join(format!(
        "{}-{}",
        std::process::id(),
        IDS.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .read(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(text.as_bytes())?;
        file.rewind()?;
        copy_file(&path, mime)
    })();
    let _ = std::fs::remove_file(path);
    result
}
