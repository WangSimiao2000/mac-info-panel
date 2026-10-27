use std::error::Error;
use std::process::ExitCode;

use panel_model::{PageModel, Section};
use panel_ui::{RendererConfig, run_page};

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
    let state = panel_core::AppState::from_config(&loaded.config, 0)?;
    let selected_page = state
        .pages
        .iter()
        .position(|page| page.id == state.selected_page)
        .ok_or("selected page disappeared after configuration validation")?;
    let descriptor = &state.pages[selected_page];
    let page = PageModel {
        schema_version: 1,
        id: descriptor.id.clone(),
        title: descriptor.title.clone(),
        icon: None,
        sections: vec![Section {
            title: None,
            widgets: Vec::new(),
        }],
        updated_at_unix_ms: None,
    };

    run_page(
        &page,
        RendererConfig {
            scale: loaded.config.appearance.scale,
            reduce_motion: loaded.config.appearance.reduce_motion,
            page_count: state.pages.len(),
            selected_page,
            ..RendererConfig::default()
        },
    )?;
    Ok(())
}
