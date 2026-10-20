//! Add viewport labels around fzf without changing its search or file records.
//! fzf's built-in jump mode only accepts single ASCII labels. A PTY lets us
//! label the rendered rows with Danish letters and longer, unambiguous codes.
use crate::Result;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::num::NonZeroU16;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZÆØÅ";
const LEFT_MARGIN: u16 = 3;

#[derive(Debug, PartialEq, Eq)]
enum Key {
    Byte(u8),
    Jump,
}

#[derive(Default)]
struct Keys {
    escape: Option<Instant>,
}

impl Keys {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Key> {
        let mut keys = Vec::new();
        for &byte in bytes {
            if self.escape.take().is_some() {
                if matches!(byte, b's' | b'S') {
                    keys.push(Key::Jump);
                    continue;
                }
                keys.push(Key::Byte(27));
            }
            if byte == 27 {
                // Alt+S is ESC followed by s; reads can split those bytes.
                self.escape = Some(Instant::now());
            } else {
                keys.push(Key::Byte(byte));
            }
        }
        keys
    }

    fn expired(&mut self) -> Option<Key> {
        if self
            .escape
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(35))
        {
            self.escape = None;
            Some(Key::Byte(27))
        } else {
            None
        }
    }
}

fn labels(count: usize) -> Vec<String> {
    let alphabet: Vec<_> = ALPHABET.chars().collect();
    let mut width = 1;
    let mut capacity = alphabet.len();
    while capacity < count {
        width += 1;
        capacity = capacity.saturating_mul(alphabet.len());
    }
    (0..count)
        .map(|mut index| {
            let mut code = vec![alphabet[0]; width];
            for letter in code.iter_mut().rev() {
                *letter = alphabet[index % alphabet.len()];
                index /= alphabet.len();
            }
            code.into_iter().collect()
        })
        .collect()
}

#[derive(Debug, PartialEq)]
struct View {
    rows: Vec<u16>,
    current: u16,
}

impl View {
    fn read(screen: &vt100::Screen, footer_lines: u16) -> Option<Self> {
        let (height, width) = screen.size();
        // The picker uses reverse layout, a one-line prompt, 1,3 margins,
        // one-line records, and a footer with a top border.
        let end = height.get().saturating_sub(footer_lines + 2);
        let rows: Vec<_> = screen
            .rows(LEFT_MARGIN + 2, width.get().saturating_sub(LEFT_MARGIN + 5))
            .enumerate()
            .filter(|(row, text)| *row >= 2 && *row < end as usize && !text.trim().is_empty())
            .map(|(row, _)| row as u16)
            .collect();
        let current = rows.iter().copied().find(|row| {
            screen
                .cell(*row, LEFT_MARGIN)
                .is_some_and(|cell| cell.contents() == ">")
        })?;
        Some(Self { rows, current })
    }
}

struct Jump {
    view: View,
    labels: Vec<String>,
    prefix: String,
    utf8: Vec<u8>,
}

impl Jump {
    fn new(view: View) -> Self {
        Self {
            labels: labels(view.rows.len()),
            view,
            prefix: String::new(),
            utf8: Vec::new(),
        }
    }

    fn draw(&self, tty: &mut File) -> io::Result<()> {
        // Save/restore the cursor without feeding the overlay back into the
        // screen parser. The empty left margin leaves filenames untouched.
        let colors = super::appearance_colors();
        write!(tty, "\x1b7\x1b[?25l\x1b[0;1m")?;
        for (channel, hex) in [(38, colors.accent), (48, colors.bg)] {
            let rgb = u32::from_str_radix(&hex[1..], 16).unwrap();
            write!(
                tty,
                "\x1b[{channel};2;{};{};{}m",
                rgb >> 16,
                (rgb >> 8) & 255,
                rgb & 255
            )?;
        }
        for (row, label) in self.view.rows.iter().zip(&self.labels) {
            write!(tty, "\x1b[{};1H", row + 1)?;
            if label.starts_with(&self.prefix) {
                write!(tty, "{label}")?;
            } else {
                write!(tty, "{}", " ".repeat(label.chars().count()))?;
            }
        }
        write!(tty, "\x1b8")?;
        tty.flush()
    }

    fn letter(&mut self, byte: u8) -> Option<char> {
        self.utf8.push(byte);
        match std::str::from_utf8(&self.utf8) {
            Ok(text) => {
                let letter = text.chars().next().map(|c| c.to_ascii_uppercase());
                self.utf8.clear();
                letter.map(|c| c.to_uppercase().next().unwrap_or(c))
            }
            Err(error) if error.error_len().is_none() => None,
            Err(_) => {
                self.utf8.clear();
                Some('\0')
            }
        }
    }
}

struct Terminal {
    tty: File,
    original: libc::termios,
}

impl Terminal {
    fn open() -> io::Result<Self> {
        let tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let mut original = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(tty.as_raw_fd(), &mut original) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut raw = original;
        unsafe { libc::cfmakeraw(&mut raw) };
        if unsafe { libc::tcsetattr(tty.as_raw_fd(), libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { tty, original })
    }

    fn size(&self) -> io::Result<libc::winsize> {
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(self.tty.as_raw_fd(), libc::TIOCGWINSZ, &mut size) } < 0 {
            return Err(io::Error::last_os_error());
        }
        size.ws_row = size.ws_row.max(1);
        size.ws_col = size.ws_col.max(1);
        Ok(size)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe { libc::tcsetattr(self.tty.as_raw_fd(), libc::TCSANOW, &self.original) };
    }
}

struct Finder(Child);

impl Drop for Finder {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn dimensions(size: &libc::winsize) -> (NonZeroU16, NonZeroU16) {
    (
        NonZeroU16::new(size.ws_row).unwrap(),
        NonZeroU16::new(size.ws_col).unwrap(),
    )
}

fn drain(master: &mut File, tty: &mut File, parser: &mut vt100::Parser) -> io::Result<()> {
    let mut output = [0; 32768];
    loop {
        match master.read(&mut output) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                parser.process(&output[..count]);
                tty.write_all(&output[..count])?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(libc::EIO) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn run(command: &mut Command, footer_lines: u16) -> Result<Output> {
    let mut terminal = Terminal::open()?;
    let mut size = terminal.size()?;
    let (mut master, mut slave) = (0, 0);
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            &size,
        )
    } < 0
    {
        return Err(io::Error::last_os_error().into());
    }
    let mut master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    // An inherited master would keep fzf alive after its terminal is closed.
    for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::from(slave.try_clone()?));
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(2, libc::TIOCSCTTY, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = Finder(command.spawn()?);
    // Command retains its configured stderr fd. Close that copy as well, so
    // the master reaches EOF when fzf exits.
    command.stderr(Stdio::inherit());
    drop(slave);
    let mut stdout = child.0.stdout.take().unwrap();
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(io::Error::last_os_error().into());
    }
    let (rows, cols) = dimensions(&size);
    let mut parser = vt100::Parser::new(rows, cols, 0);
    let mut jump: Option<Jump> = None;
    let mut keys = Keys::default();
    let status = loop {
        let new_size = terminal.size()?;
        if (new_size.ws_row, new_size.ws_col) != (size.ws_row, size.ws_col) {
            let had_labels = jump.take().is_some();
            size = new_size;
            let (rows, cols) = dimensions(&size);
            parser.set_size(rows, cols);
            if had_labels {
                terminal
                    .tty
                    .write_all(parser.screen().state_formatted().as_bytes())?;
            }
            // fzf redraws after TIOCSWINSZ sends SIGWINCH to its foreground group.
            unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) };
        }
        let mut fds = [
            libc::pollfd {
                fd: master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: terminal.tty.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 40) } < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if fds[0].revents != 0 {
            drain(&mut master, &mut terminal.tty, &mut parser)?;
            if let Some(active) = &mut jump {
                if let Some(view) = View::read(parser.screen(), footer_lines)
                    && view.rows == active.view.rows
                {
                    active.view = view;
                    active.draw(&mut terminal.tty)?;
                } else {
                    jump = None;
                    terminal
                        .tty
                        .write_all(parser.screen().state_formatted().as_bytes())?;
                }
            }
        }
        if let Some(status) = child.0.try_wait()? {
            // Include the final frame (especially --no-clear in resident mode).
            drain(&mut master, &mut terminal.tty, &mut parser)?;
            break status;
        }
        let mut events: Vec<_> = keys.expired().into_iter().collect();
        if fds[1].revents & libc::POLLIN != 0 {
            let mut input = [0; 4096];
            let count = terminal.tty.read(&mut input)?;
            if count == 0 {
                return Err("Picker terminal closed".into());
            }
            events.extend(keys.feed(&input[..count]));
        }
        for key in events {
            if key == Key::Jump {
                if jump.take().is_some() {
                    terminal
                        .tty
                        .write_all(parser.screen().state_formatted().as_bytes())?;
                } else if let Some(view) = View::read(parser.screen(), footer_lines) {
                    let active = Jump::new(view);
                    active.draw(&mut terminal.tty)?;
                    jump = Some(active);
                }
                continue;
            }
            let Key::Byte(byte) = key else { unreachable!() };
            let Some(active) = &mut jump else {
                master.write_all(&[byte])?;
                continue;
            };
            if matches!(byte, 3 | 27) {
                jump = None;
                terminal
                    .tty
                    .write_all(parser.screen().state_formatted().as_bytes())?;
                master.write_all(&[byte])?;
                continue;
            }
            if matches!(byte, 8 | 127) {
                active.prefix.pop();
                active.utf8.clear();
                active.draw(&mut terminal.tty)?;
                continue;
            }
            let Some(letter) = active.letter(byte) else {
                continue;
            };
            active.prefix.push(letter);
            let exact = active
                .labels
                .iter()
                .position(|label| label == &active.prefix);
            let valid = active
                .labels
                .iter()
                .any(|label| label.starts_with(&active.prefix));
            if exact.is_some() || !valid {
                if let Some(index) = exact {
                    let distance = active.view.rows[index] as i32 - active.view.current as i32;
                    let key = if distance < 0 { b"\x1b[A" } else { b"\x1b[B" };
                    for _ in 0..distance.unsigned_abs() {
                        master.write_all(key)?;
                    }
                }
                jump = None;
                terminal
                    .tty
                    .write_all(parser.screen().state_formatted().as_bytes())?;
            } else {
                active.draw(&mut terminal.tty)?;
            }
        }
    };
    Ok(Output {
        status,
        stdout: output
            .join()
            .map_err(|_| "Could not read picker selection")??,
        stderr: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_s_works_across_reads_and_other_keys_pass_through() {
        let mut keys = Keys::default();
        assert!(keys.feed(b"\x1b").is_empty());
        assert_eq!(keys.feed(b"s"), [Key::Jump]);
        assert_eq!(keys.feed(b"\x1bS"), [Key::Jump]);
        assert_eq!(keys.feed(b"\x03"), [Key::Byte(3)]);
        assert_eq!(
            keys.feed(b"\x1b[B"),
            [Key::Byte(27), Key::Byte(b'['), Key::Byte(b'B')]
        );
        keys.feed(b"\x1b");
        keys.escape = Some(Instant::now() - Duration::from_millis(40));
        assert_eq!(keys.expired(), Some(Key::Byte(27)));
        assert_eq!(keys.feed(b"s"), [Key::Byte(b's')]);
    }

    #[test]
    fn labels_expand_without_ambiguous_prefixes() {
        assert_eq!(labels(3), ["A", "B", "C"]);
        let full = labels(29);
        assert_eq!(&full[26..], ["Æ", "Ø", "Å"]);
        let large = labels(31);
        assert_eq!(&large[..3], ["AA", "AB", "AC"]);
        assert_eq!(&large[26..], ["AÆ", "AØ", "AÅ", "BA", "BB"]);
        assert_eq!(labels(842).last().unwrap(), "BAA");
    }

    #[test]
    fn danish_labels_accept_split_utf8_and_lowercase() {
        let mut active = Jump::new(View {
            rows: vec![2],
            current: 2,
        });
        assert_eq!(active.letter(0xc3), None);
        assert_eq!(active.letter(0xa6), Some('Æ'));
        assert_eq!(active.letter(b'a'), Some('A'));
    }
}
