//! Shows a notebook with images, to check each protocol on a real terminal.

fn main() -> std::process::ExitCode {
    europa_tui::run_image_probe(std::env::args_os().skip(1))
}
