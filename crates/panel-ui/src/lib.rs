pub mod design_tokens {
    include!(concat!(env!("OUT_DIR"), "/design_tokens.rs"));
}
pub mod renderer;
pub mod view_model;

slint::include_modules!();

pub use renderer::{RendererConfig, UiError, apply_payload, build_payload, run_page, set_page};
