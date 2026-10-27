use file_picker::{
    Action, HELP, Options, Result, checked_files, copy_selection, drag_binary, notify,
};
use std::env;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

fn run() -> Result<()> {
    let options = Options::parse(env::args_os().skip(1))?;
    match &options.action {
        Action::Pick => file_picker::pick(&options),
        Action::List => file_picker::list_files(&options),
        Action::CachedList => file_picker::index::list(&options),
        Action::IndexUpdate => file_picker::index::update(&options, true),
        Action::IndexUpdateIfIdle => file_picker::index::update(&options, false),
        Action::IndexStatus => file_picker::index::status(),
        Action::IndexRemove(paths) => file_picker::index::remove(paths),
        Action::IndexClear => file_picker::index::clear(&options),
        Action::IndexStream => file_picker::index::stream(&options),
        Action::Copy(paths) => copy_selection(&checked_files(paths.clone())?, false),
        Action::Path(paths) => copy_selection(&checked_files(paths.clone())?, true),
        Action::Drag(paths) => Err(Command::new(drag_binary()?)
            .args(checked_files(paths.clone())?)
            .exec()
            .into()),
        Action::LatestDownload => file_picker::paste_latest_download(),
        Action::Window => file_picker::window(&options),
        Action::PrepareTerminal => file_picker::prepare_terminal(),
        Action::Help => {
            print!("{HELP}");
            Ok(())
        }
        Action::Version => {
            println!("file-picker {} (Ratatui)", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("file-picker: {error}");
            // Background helpers report through their caller, without desktop popups.
            if !std::env::args_os().any(|arg| {
                matches!(
                    arg.to_str(),
                    Some("--index-stream" | "--index-update-if-idle")
                )
            }) {
                notify("File picker", &error.to_string(), true);
            }
            ExitCode::FAILURE
        }
    }
}
