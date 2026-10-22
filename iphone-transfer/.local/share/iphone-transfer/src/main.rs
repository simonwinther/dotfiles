fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if let Err(error) = iphone_transfer::run(&args) {
        eprintln!("iphone-transfer: {error:#}");
        std::process::exit(1);
    }
}
