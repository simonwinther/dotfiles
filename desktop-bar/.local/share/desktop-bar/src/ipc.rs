use crate::{modules::Update, theme};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::{
    io,
    os::unix::{fs::PermissionsExt, net::UnixDatagram},
    path::PathBuf,
};

pub fn send(panel: &str) -> io::Result<()> {
    UnixDatagram::unbound()?
        .send_to(panel.as_bytes(), theme::runtime().join("desktop/bar.sock"))?;
    Ok(())
}

pub struct Server(PathBuf);
impl Server {
    pub fn start(sender: Sender<Update>) -> io::Result<Self> {
        let directory = theme::runtime().join("desktop");
        std::fs::create_dir_all(&directory)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        let path = directory.join("bar.sock");
        if path.exists() {
            if send("ping").is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "a desktop bar is already running",
                ));
            }
            std::fs::remove_file(&path)?;
        }
        let socket = UnixDatagram::bind(&path)?;
        std::thread::Builder::new()
            .name("bar-ipc".into())
            .spawn(move || {
                let mut buffer = [0; 16 * 1024];
                loop {
                    let length = match socket.recv(&mut buffer) {
                        Ok(length) => length,
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        // Keep serving after transient failures such as fd exhaustion.
                        Err(error) => {
                            eprintln!("desktop-bar: command socket: {error}");
                            std::thread::sleep(std::time::Duration::from_millis(250));
                            continue;
                        }
                    };
                    if let Ok(command) = std::str::from_utf8(&buffer[..length])
                        && command != "ping"
                        && sender
                            .send(Update::Command(
                                command.into(),
                                crate::modules::workspaces::focused_output(),
                            ))
                            .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self(path))
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
