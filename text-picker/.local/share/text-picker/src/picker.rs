use crate::Result;
use crate::config::Entry;
use crate::shell_quote;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn display(text: &str) -> String {
    text.replace(['\n', '\r', '\t'], " ")
}

pub fn input(entries: &[Entry]) -> String {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            format!(
                "{index}\t{}\t{}\n",
                display(&entry.label),
                display(&entry.text)
            )
        })
        .collect()
}

pub fn index(output: &[u8], count: usize) -> Result<usize> {
    let output = std::str::from_utf8(output)?;
    let index: usize = output
        .split('\t')
        .next()
        .unwrap_or_default()
        .trim()
        .parse()?;
    if index >= count {
        return Err("The picker returned an unknown entry".into());
    }
    Ok(index)
}

pub fn preview(path: &Path, index: usize) -> Result<()> {
    let entries = crate::config::read(path)?;
    let entry = entries.get(index).ok_or("Unknown preview entry")?;
    print!("{}", entry.text);
    Ok(())
}

fn command(executable: &Path, data: &Path) -> Command {
    let preview = format!(
        "{} --preview {} {{1}}",
        shell_quote(executable.as_os_str()),
        shell_quote(data.as_os_str())
    );
    let mut command = Command::new("fzf");
    command
        .args([
            "--delimiter=\t",
            "--with-nth=2..",
            "--no-multi",
            "--no-print-query",
            "--no-expect",
            "--accept-nth=1..",
            "--reverse",
            "--border=rounded",
            "--margin=5%",
            "--prompt=Search > ",
            "--color=fg:white,fg+:white,bg+:-1",
            "--color=hl:green,hl+:bright-green",
            "--color=border:green,prompt:green,pointer:green,marker:green,spinner:green,info:gray",
            "--preview-window=right:45%:wrap",
            "--tabstop=2",
            "--separator= │ ",
            "--no-hscroll",
        ])
        .arg(format!("--preview={preview}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    command
}

pub fn choose(entries: &[Entry], executable: &Path) -> Result<Option<usize>> {
    if entries.is_empty() {
        return Err("No entries are configured for this picker".into());
    }
    let snapshot = tempfile::Builder::new()
        .prefix("text-picker-preview-")
        .tempdir()?;
    let data = snapshot.path().join("entries.json");
    fs::write(&data, serde_json::to_vec(entries)?)?;
    let mut child = command(executable, &data).spawn()?;
    let write = child
        .stdin
        .take()
        .ok_or("Could not open picker input")?
        .write_all(input(entries).as_bytes());
    let output = child.wait_with_output()?;
    if matches!(output.status.code(), Some(1 | 130)) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(format!("fzf exited with {}", output.status).into());
    }
    write?;
    Ok(Some(index(&output.stdout, entries.len())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn record_ids_keep_duplicate_labels_and_multiline_payloads_distinct() -> Result<()> {
        let entries = vec![
            Entry {
                label: "Repeated".into(),
                text: "first\nline".into(),
            },
            Entry {
                label: "Repeated".into(),
                text: "second\tline".into(),
            },
        ];
        assert_eq!(
            input(&entries),
            "0\tRepeated\tfirst line\n1\tRepeated\tsecond line\n"
        );
        assert_eq!(index(b"1\tRepeated\tsecond line\n", 2)?, 1);
        assert!(index(b"9\tunknown\n", 2).is_err());
        assert_eq!(entries[1].text, "second\tline");
        Ok(())
    }

    #[test]
    fn preview_command_quotes_paths_including_spaces_and_apostrophes() -> Result<()> {
        let value = OsStr::new("/some path/it's $(literal)");
        let output = Command::new("sh")
            .args(["-c", &format!("printf %s {}", shell_quote(value))])
            .output()?;
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout)?, value.to_string_lossy());
        Ok(())
    }

    #[test]
    fn actual_fzf_options_search_labels_and_return_the_original_record_id() -> Result<()> {
        let entries = vec![
            Entry {
                label: "Needle".into(),
                text: "first value".into(),
            },
            Entry {
                label: "Other".into(),
                text: "second value".into(),
            },
        ];
        let mut child = command(Path::new("unused-preview"), Path::new("unused-data"))
            .arg("--filter=Needle")
            .spawn()?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input(&entries).as_bytes())?;
        let output = child.wait_with_output()?;
        assert!(output.status.success());
        assert_eq!(index(&output.stdout, entries.len())?, 0);
        Ok(())
    }
}
