//! Keep one independent pill on each enabled output.

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::mpsc;
use std::time::Duration;

use crate::hypr;

#[derive(Default)]
struct Pills(HashMap<String, Child>);

impl Pills {
    /// Returns false when startup or hotplug needs a short retry.
    fn reconcile(&mut self) -> bool {
        let Some(outputs) = hypr::monitor_names() else {
            return false;
        };
        self.0.retain(|name, child| {
            if !outputs.contains(name) {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            matches!(child.try_wait(), Ok(None))
        });

        let Ok(executable) = std::env::current_exe() else {
            return false;
        };
        let mut ready = true;
        for name in outputs {
            if self.0.contains_key(&name) {
                continue;
            }
            let mut command = Command::new(&executable);
            command.env("WAYBAR_OUTPUT_NAME", &name);
            // A normal restart or a killed supervisor must not leave orphan
            // pills behind. Also cover the race where it exits before exec.
            let parent = std::process::id() as libc::pid_t;
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        return Err(std::io::Error::other("slider supervisor exited"));
                    }
                    Ok(())
                });
            }
            match command.spawn() {
                Ok(child) => {
                    self.0.insert(name, child);
                    // Check once after startup in case an output was not yet
                    // visible to Wayland when the child connected.
                    ready = false;
                }
                Err(error) => {
                    eprintln!("cannot start pill on {name}: {error}");
                    ready = false;
                }
            }
        }
        ready
    }
}

impl Drop for Pills {
    fn drop(&mut self) {
        for child in self.0.values_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn run() {
    let (sender, receiver) = mpsc::channel();
    hypr::listen(move |event| {
        // The supervisor only needs output events; individual pills handle
        // workspace and appearance changes themselves.
        if matches!(event, hypr::Event::OutputsChanged) {
            let _ = sender.send(());
        }
    });
    let mut pills = Pills::default();
    loop {
        let ready = pills.reconcile();
        let retry = if ready { crate::FALLBACK_REFRESH } else { Duration::from_secs(1) };
        match receiver.recv_timeout(retry) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}
