use std::env;
use std::path::Path;
use text_picker::Result;
use text_picker::config::{self, Mode};
use text_picker::{handoff, hyprland::Hyprland, picker};

const HELP: &str = "Snippet and macro pickers\n\nUsage: text-picker snippets|macros [--list]\n\n  snippets  Search snippets and paste the selected text\n  macros    Search macros and paste the selected text\n  --list    List labels without opening a picker or touching the clipboard\n  -h, --help  Show this help\n\nThe globalsnip and macro-picker commands select the matching mode automatically.";

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => println!("{HELP}"),
        [flag, file, id] if flag == "--preview" => {
            picker::preview(
                Path::new(file),
                id.to_str().ok_or("Invalid entry ID")?.parse()?,
            )?;
        }
        [flag, path] if flag == "--handoff" => handoff::serve(Path::new(path))?,
        [mode, rest @ ..] => {
            if rest == ["--help"] || rest == ["-h"] {
                println!("{HELP}");
                return Ok(());
            }
            let mode = Mode::parse(mode.to_str().ok_or("Invalid picker mode")?)?;
            let entries = config::read(&config::directory()?.join(mode.filename()))?;
            if rest == ["--list"] {
                for entry in entries {
                    println!("{}", entry.label);
                }
            } else if rest.is_empty() {
                let hyprland = Hyprland::discover()?;
                hyprland.active()?;
                let executable = env::current_exe()?;
                if let Some(id) = picker::choose(&entries, &executable)? {
                    handoff::send(&hyprland, &executable, &entries[id].text)?;
                }
            } else {
                return Err(HELP.into());
            }
        }
        _ => return Err(HELP.into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("text-picker: {error}");
        if env::args_os()
            .nth(1)
            .is_none_or(|argument| argument != "--handoff" && argument != "--preview")
        {
            text_picker::notify(&error.to_string());
        }
        std::process::exit(1);
    }
}
