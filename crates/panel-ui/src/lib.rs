pub mod view_model;

slint::include_modules!();

pub fn run() -> Result<(), slint::PlatformError> {
    let window = AppWindow::new()?;
    window.run()
}
