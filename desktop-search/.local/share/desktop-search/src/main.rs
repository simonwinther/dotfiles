mod applications;
mod backend;
mod client;
mod collector;
mod config;
mod edits;
mod events;
#[path = "../../../../../desktop-bar/.local/share/desktop-bar/src/fuzzy.rs"]
mod fuzzy;
mod launch;
mod migration;
#[path = "../../../../../desktop-bar/.local/share/desktop-bar/src/process.rs"]
mod process;
mod protocol;
mod providers;
mod server;
mod store;
mod todo;

fn reset_child_signals() {
    unsafe {
        let mut mask: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut mask);
        libc::sigprocmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut());
    }
}

use anyhow::{Result, ensure};
use std::{
    fs::OpenOptions,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    sync::Arc,
};

fn main() {
    unsafe {
        libc::umask(0o077);
        #[cfg(target_env = "gnu")]
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
    if let Err(error) = run() {
        eprintln!("desktop-search: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let paths = config::Paths::environment()?;
    match arguments.first().map(String::as_str).unwrap_or("serve") {
        "serve" => {
            ensure!(
                arguments.iter().skip(1).all(|argument| matches!(
                    argument.as_str(),
                    "--no-collector" | "--no-compatibility"
                )),
                "Unknown service option"
            );
            config::private_directory(&paths.state)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(paths.state.join("service.lock"))?;
            ensure!(
                unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
                "A search service already owns this history"
            );
            let config = config::Config::load(&paths)?;
            let (backend, wake) = backend::Backend::new(paths, config)?;
            start_reminders(backend.clone())?;
            edits::start(backend.clone())?;
            let collect = !arguments
                .iter()
                .any(|argument| argument == "--no-collector");
            server::serve(
                backend,
                !arguments
                    .iter()
                    .any(|argument| argument == "--no-compatibility"),
                collect.then_some(wake),
            )
        }
        "query" => {
            let provider = arguments
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("query requires a provider"))?;
            let query = arguments[2..].join(" ");
            let mut items = vec![];
            client::query(
                &paths.socket,
                &protocol::Query {
                    providers: vec![provider.clone()],
                    query,
                    maxresults: 1000,
                    ..Default::default()
                },
                |item| {
                    items.push(item);
                    Ok(())
                },
            )?;
            println!("{}", serde_json::to_string(&items)?);
            Ok(())
        }
        "catalog" => {
            ensure!(
                arguments.len() == 2 && arguments[1] == "apps",
                "catalog requires apps"
            );
            let mut entries = vec![];
            client::query(
                &paths.socket,
                &protocol::Query {
                    providers: vec!["desktopapplications".into()],
                    maxresults: 20_000,
                    ..Default::default()
                },
                |item| {
                    let image = if std::path::Path::new(&item.icon).is_absolute() {
                        item.icon
                    } else {
                        String::new()
                    };
                    entries.push(serde_json::json!({"id":item.identifier,"name":item.text,"detail":item.subtext,"keywords":item.preview,"image":image,"command":["bar","launch",item.identifier]}));
                    Ok(())
                },
            )?;
            println!("{}", serde_json::to_string(&entries)?);
            Ok(())
        }
        "listproviders" => {
            for name in providers::NAMES {
                if *name != "menus" {
                    println!("{name}");
                }
            }
            for name in ["notifications", "repositories"] {
                println!("menus:{name}");
            }
            let menus = std::env::var_os("DESKTOP_MENUS_FILE")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| paths.config.join("desktop/menus.json"));
            if let Ok(file) = std::fs::read(menus)
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&file)
                && let Some(menus) = value.as_object()
            {
                for name in menus.keys() {
                    println!("menus:{name}");
                }
            }
            Ok(())
        }
        "activate" => {
            ensure!(
                arguments.len() >= 4,
                "activate requires PROVIDER IDENTIFIER ACTION [QUERY]"
            );
            let result = client::activate(
                &paths.socket,
                &protocol::Activation {
                    provider: arguments[1].clone(),
                    identifier: arguments[2].clone(),
                    action: arguments[3].clone(),
                    query: arguments[4..].join(" "),
                    ..Default::default()
                },
            )?;
            println!("{result}");
            Ok(())
        }
        "status" => {
            println!(
                "{}",
                serde_json::to_string(&client::state(&paths.socket, "desktop-search")?)?
            );
            Ok(())
        }
        "import-elephant" => {
            let result = client::activate(
                &paths.socket,
                &protocol::Activation {
                    provider: "desktop-search".into(),
                    action: "import-elephant".into(),
                    ..Default::default()
                },
            )?;
            println!("{result}");
            Ok(())
        }
        "--help" | "help" => {
            println!(
                "desktop-search [serve [--no-collector] [--no-compatibility]]\ndesktop-search query PROVIDER [QUERY]\ndesktop-search catalog apps\ndesktop-search activate PROVIDER IDENTIFIER ACTION [QUERY]\ndesktop-search import-elephant\ndesktop-search status"
            );
            Ok(())
        }
        _ => anyhow::bail!("Unknown command; see --help"),
    }
}

fn start_reminders(backend: Arc<backend::Backend>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("search-reminders".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let Ok(listener) = backend.events.listen("todo".into()) else {
                return;
            };
            loop {
                let mut failed = false;
                if let Ok(items) = todo::due(&backend) {
                    for (id, text, urgency) in items {
                        let result = process::output(
                            std::process::Command::new("notify-send").args([
                                "--urgency",
                                &urgency,
                                "Task reminder",
                                &text,
                            ]),
                            std::time::Duration::from_secs(3),
                            4096,
                            || false,
                        );
                        if result.is_ok_and(|output| output.status.success()) {
                            let _ = backend
                                .store
                                .lock()
                                .unwrap()
                                .db
                                .execute("UPDATE todos SET notified=1 WHERE id=?1", [id]);
                        } else {
                            failed = true;
                        }
                    }
                }
                let next: Option<i64> = backend
                    .store
                    .lock()
                    .unwrap()
                    .db
                    .query_row(
                        "SELECT MIN(scheduled) FROM todos WHERE notified=0 AND state!='done'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap_or(None);
                let timeout = if failed {
                    30_000
                } else {
                    next.map(|time| {
                        (time - chrono::Local::now().timestamp())
                            .max(1)
                            .saturating_mul(1000)
                            .min(i32::MAX as i64) as i32
                    })
                    .unwrap_or(-1)
                };
                let mut fd = libc::pollfd {
                    fd: listener.fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                unsafe {
                    libc::poll(&mut fd, 1, timeout);
                }
                listener.take();
            }
        })?;
    Ok(())
}
