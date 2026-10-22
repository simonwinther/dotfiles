use std::io::{self, Write};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("--native-host") {
        if let Err(error) =
            dictation_volume_duck::native::serve(&mut io::stdin().lock(), &mut io::stdout().lock())
        {
            eprintln!("dictation-volume-duck host: {error:#}");
            std::process::exit(1);
        }
    } else {
        let result = dictation_volume_duck::cli(&args);
        let _ = io::stdout().write_all(result.stdout.as_bytes());
        let _ = io::stderr().write_all(result.stderr.as_bytes());
        std::process::exit(result.code);
    }
}
