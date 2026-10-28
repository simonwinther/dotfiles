//! Incremental OSC 52 output. The caller must suspend drawing until this finishes.
use anyhow::{Context, Result, bail};
use crossterm::{Command, clipboard::CopyToClipboard};
use std::{
    fs::{File, OpenOptions},
    io::{self, IsTerminal, Write},
    os::unix::fs::OpenOptionsExt,
};

const BUDGET: usize = 64 * 1024;
const TERMINATOR: &[u8] = b"\x1b\\";
// mytop targets Linux Slurm hosts. Opening again creates a separate open-file description,
// so O_NONBLOCK does not change the flags of ratatui's stdout or the shell's terminal.
const O_NONBLOCK: i32 = 0o4000;

pub struct Transfer {
    tty: File,
    packet: Vec<u8>,
    offset: usize,
    original_bytes: usize,
    started: bool,
    cancelled: bool,
}

impl Transfer {
    pub fn start(text: String) -> Result<Self> {
        if !io::stdout().is_terminal() {
            bail!("Clipboard output requires a terminal");
        }
        let tty = OpenOptions::new()
            .write(true)
            .custom_flags(O_NONBLOCK)
            .open("/proc/self/fd/1")
            .or_else(|_| {
                OpenOptions::new()
                    .write(true)
                    .custom_flags(O_NONBLOCK)
                    .open("/dev/tty")
            })
            .context("Opening terminal for clipboard output")?;
        Self::with_tty(text, tty)
    }

    fn with_tty(text: String, tty: File) -> Result<Self> {
        let original_bytes = text.len();
        let capacity = original_bytes
            .div_ceil(3)
            .saturating_mul(4)
            .saturating_add(16);
        let mut packet = String::with_capacity(capacity);
        CopyToClipboard::to_clipboard_from(text)
            .write_ansi(&mut packet)
            .context("Encoding clipboard output")?;
        Ok(Self {
            tty,
            packet: packet.into_bytes(),
            offset: 0,
            original_bytes,
            started: false,
            cancelled: false,
        })
    }

    pub fn bytes(&self) -> usize {
        self.original_bytes
    }

    /// Write at most 64 KiB without blocking. `true` means the complete OSC was sent.
    pub fn advance(&mut self) -> Result<bool> {
        let before = self.offset;
        let result = advance_packet(&mut self.tty, &self.packet, &mut self.offset, BUDGET);
        self.started |= self.offset > before;
        result.context("Sending clipboard output")
    }

    /// Stop sending data and finish the escape sequence before the caller redraws.
    /// A terminal may copy the prefix already received; cancellation is not success.
    pub fn cancel(&mut self) {
        if self.cancelled || self.offset == self.packet.len() {
            return;
        }
        self.cancelled = true;
        self.packet.clear();
        if self.started {
            self.packet.extend_from_slice(TERMINATOR);
        }
        self.offset = 0;
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        if self.started && self.offset < self.packet.len() {
            let mut offset = 0;
            let _ = advance_packet(&mut self.tty, TERMINATOR, &mut offset, TERMINATOR.len());
        }
    }
}

fn advance_packet(
    writer: &mut impl Write,
    packet: &[u8],
    offset: &mut usize,
    budget: usize,
) -> io::Result<bool> {
    let end = offset.saturating_add(budget).min(packet.len());
    while *offset < end {
        match writer.write(&packet[*offset..end]) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => *offset += written,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    Ok(*offset == packet.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, os::fd::OwnedFd, os::unix::net::UnixStream};

    fn transfer(text: String) -> (Transfer, UnixStream) {
        let (writer, reader) = UnixStream::pair().unwrap();
        writer.set_nonblocking(true).unwrap();
        reader.set_nonblocking(true).unwrap();
        let writer = File::from(OwnedFd::from(writer));
        (Transfer::with_tty(text, writer).unwrap(), reader)
    }

    fn drain(reader: &mut UnixStream, output: &mut Vec<u8>) {
        let mut buffer = [0; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => output.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return,
                Err(error) => panic!("{error}"),
            }
        }
    }

    #[test]
    fn transfer_handles_backpressure_and_sends_the_exact_osc_in_bounded_steps() {
        let text = "some log output\n".repeat(BUDGET);
        let (mut transfer, mut reader) = transfer(text.clone());
        assert_eq!(transfer.bytes(), text.len());
        let expected = transfer.packet.clone();
        let mut stalled = false;
        for _ in 0..64 {
            let previous = transfer.offset;
            assert!(!transfer.advance().unwrap());
            assert!(transfer.offset - previous <= BUDGET);
            if previous == transfer.offset {
                stalled = true;
                break;
            }
        }
        assert!(
            stalled,
            "socket must apply backpressure before output is drained"
        );
        let mut output = Vec::new();
        loop {
            drain(&mut reader, &mut output);
            let previous = transfer.offset;
            let done = transfer.advance().unwrap();
            assert!(transfer.offset - previous <= BUDGET);
            if done {
                break;
            }
        }
        drain(&mut reader, &mut output);
        assert_eq!(output, expected);
        assert!(output.starts_with(b"\x1b]52;c;"));
        assert!(output.ends_with(TERMINATOR));
    }

    #[test]
    fn cancellation_finishes_the_osc_without_sending_the_remaining_file() {
        let (mut transfer, mut reader) = transfer("x".repeat(BUDGET * 4));
        assert!(!transfer.advance().unwrap());
        let mut expected = transfer.packet[..transfer.offset].to_vec();
        expected.extend_from_slice(TERMINATOR);
        transfer.cancel();
        assert!(transfer.advance().unwrap());
        let mut output = Vec::new();
        drain(&mut reader, &mut output);
        assert_eq!(output, expected);
    }

    #[test]
    fn cancellation_before_output_sends_nothing() {
        let (mut transfer, mut reader) = transfer("hello".into());
        transfer.cancel();
        assert!(transfer.advance().unwrap());
        let mut output = Vec::new();
        drain(&mut reader, &mut output);
        assert!(output.is_empty());
    }

    #[test]
    fn repeated_cancellation_preserves_a_partly_written_terminator() {
        struct OneByte(bool);
        impl Write for OneByte {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0 {
                    Err(io::ErrorKind::WouldBlock.into())
                } else {
                    self.0 = true;
                    Ok(bytes.len().min(1))
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let (mut transfer, _reader) = transfer("x".repeat(BUDGET * 4));
        assert!(!transfer.advance().unwrap());
        transfer.cancel();
        assert!(
            !advance_packet(
                &mut OneByte(false),
                &transfer.packet,
                &mut transfer.offset,
                BUDGET
            )
            .unwrap()
        );
        assert_eq!(transfer.offset, 1);
        transfer.cancel();
        assert_eq!(&transfer.packet[transfer.offset..], b"\\");
    }

    #[test]
    fn dropping_an_incomplete_transfer_attempts_to_close_the_osc() {
        let (mut transfer, mut reader) = transfer("x".repeat(BUDGET * 4));
        assert!(!transfer.advance().unwrap());
        let mut expected = transfer.packet[..transfer.offset].to_vec();
        expected.extend_from_slice(TERMINATOR);
        drop(transfer);
        let mut output = Vec::new();
        drain(&mut reader, &mut output);
        assert_eq!(output, expected);
    }
}
