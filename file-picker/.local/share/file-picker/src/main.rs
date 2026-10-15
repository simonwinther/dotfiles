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
        Action::Copy(paths) => copy_selection(&checked_files(paths.clone())?, false),
        Action::Path(paths) => copy_selection(&checked_files(paths.clone())?, true),
        Action::Drag(paths) => Err(Command::new(drag_binary()?)
            .args(checked_files(paths.clone())?)
            .exec()
            .into()),
        Action::LatestDownload => file_picker::paste_latest_download(),
        Action::Window => file_picker::window(&options, false),
        Action::Warm => file_picker::window(&options, true),
        Action::Resident => file_picker::resident(&options),
        Action::Help => {
            print!("{HELP}");
            Ok(())
        }
        Action::Version => {
            println!("file-picker {} (Rust)", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("file-picker: {error}");
            notify("File picker", &error.to_string(), true);
            ExitCode::FAILURE
        }
    }
}
