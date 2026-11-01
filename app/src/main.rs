// Desktop and iOS entry point; Android starts in `android_main` instead.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() -> Result<(), slint::PlatformError> {
    // `--data-dir <path>` points the desktop build at another data folder, and `--seed` replaces
    // its data with the demo dataset (handy for testing; never use it on real data).
    let mut args = std::env::args().skip(1);
    let mut data_dir = None;
    let mut seed = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "--seed" => seed = true,
            _ => {}
        }
    }
    project_app::run(data_dir.unwrap_or_else(project_app::default_data_dir), seed)
}
