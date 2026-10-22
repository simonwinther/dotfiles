use std::process::{Command, Output};
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plot-native"))
        .args(args)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}
#[test]
fn help_preserves_formula_and_range_interface() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("--range <MIN> <MAX>"));
    assert!(text.contains("sin(x), cos(x)"));
}
#[test]
fn headless_png_accepts_the_old_split_formula_arguments() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("curves.png");
    let output = run(&[
        "y=",
        "2x",
        "+",
        "1, sin(x)",
        "-r",
        "-5",
        "5",
        "--output",
        path.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(&std::fs::read(path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
}
#[test]
fn malformed_formula_is_reported_while_valid_curves_still_export() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("curves.svg");
    let output = run(&["bad(x), sin(x)", "--output", path.to_str().unwrap()]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown function"));
    assert!(std::fs::read_to_string(path).unwrap().contains("sin(x)"));
}
#[test]
fn invalid_ranges_and_all_invalid_formulas_fail_before_creating_output() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("curves.svg");
    for args in [
        vec!["x", "-r", "5", "-5"],
        vec!["x", "-r", "nan", "10"],
        vec!["bad(x)"],
        vec!["x", "--width", "1"],
    ] {
        let mut args = args;
        args.extend(["--output", path.to_str().unwrap()]);
        let output = run(&args);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
}
#[test]
fn function_arguments_do_not_split_curves() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("curves.svg");
    let output = run(&[
        "clip(x,-1,1), where(x<0,0,x)",
        "--output",
        path.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let svg = std::fs::read_to_string(path).unwrap();
    assert!(svg.contains("clip(x,-1,1)"));
    assert!(svg.contains("where(x&lt;0,0,x)"));
}
#[test]
fn missing_formulas_and_unsupported_file_types_fail_cleanly() {
    assert!(!run(&[]).status.success());
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("curves.jpg");
    assert!(
        !run(&["x", "--output", path.to_str().unwrap()])
            .status
            .success()
    );
    assert!(!path.exists());
}

#[test]
fn headless_pdf_has_a_valid_header() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("curves.pdf");
    let output = run(&["sin(x), x^2", "--output", path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(std::fs::read(path).unwrap().starts_with(b"%PDF-"));
}
