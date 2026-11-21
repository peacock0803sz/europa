//! Terminal interface for Europa.

fn main() -> std::process::ExitCode {
    europa_tui::run(std::env::args_os())
}
