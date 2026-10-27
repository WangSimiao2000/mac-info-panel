use std::error::Error;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let loaded = panel_config::load_or_create_default()?;
    let _state = panel_core::AppState::from_config(&loaded.config, 0)?;
    panel_ui::run()?;
    Ok(())
}
