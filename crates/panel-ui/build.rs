use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TokenFile {
    schema_version: u32,
    colors: ColorTokens,
    typography: TypographyTokens,
    spacing: SpacingTokens,
    radius: RadiusTokens,
    elevation: ElevationTokens,
    motion: MotionTokens,
    breakpoints: BreakpointTokens,
    layout: LayoutTokens,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ColorTokens {
    background: String,
    surface: String,
    surface_elevated: String,
    foreground: String,
    secondary_foreground: String,
    accent: String,
    success: String,
    warning: String,
    danger: String,
    shadow: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TypographyTokens {
    label: f32,
    body: f32,
    title: f32,
    value: f32,
    regular_weight: u16,
    emphasis_weight: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SpacingTokens {
    small: f32,
    medium: f32,
    large: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RadiusTokens {
    card: f32,
    panel: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ElevationTokens {
    card_shadow_blur: f32,
    card_shadow_opacity: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MotionTokens {
    fast_milliseconds: u32,
    page_milliseconds: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BreakpointTokens {
    medium: f32,
    large: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LayoutTokens {
    window_width: f32,
    window_height: f32,
    min_widget_width: f32,
    min_widget_height: f32,
    text_widget_height: f32,
    chart_widget_height: f32,
    empty_section_height: f32,
    list_base_height: f32,
    list_line_height: f32,
    section_header_height: f32,
    header_height: f32,
    footer_height: f32,
    status_indicator_size: f32,
    gauge_track_height: f32,
    progress_track_height: f32,
    heading_gap: f32,
    chart_bar_border_width: f32,
    indicator_width: f32,
    indicator_height: f32,
    chrome_height: f32,
    text_max_lines: usize,
    max_text_chars: usize,
    max_list_items: usize,
    max_chart_samples: usize,
    max_visible_indicators: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing CARGO_MANIFEST_DIR")?);
    let token_path = manifest_dir.join("../../themes/default/tokens.json");
    println!("cargo:rerun-if-changed={}", token_path.display());

    let tokens: TokenFile = serde_json::from_str(&fs::read_to_string(&token_path)?)?;
    validate_tokens(&tokens)?;

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let slint_path = out_dir.join("theme.slint");
    let rust_path = out_dir.join("design_tokens.rs");
    fs::write(&slint_path, slint_source(&tokens))?;
    fs::write(&rust_path, rust_source(&tokens))?;

    let libraries = HashMap::from([("theme".to_owned(), slint_path)]);
    let config = slint_build::CompilerConfiguration::new().with_library_paths(libraries);
    slint_build::compile_with_config("ui/app-window.slint", config)?;
    Ok(())
}

fn validate_tokens(tokens: &TokenFile) -> Result<(), Box<dyn Error>> {
    if tokens.schema_version != 1 {
        return Err(format!("unsupported design token schema {}", tokens.schema_version).into());
    }
    for (name, color) in [
        ("background", &tokens.colors.background),
        ("surface", &tokens.colors.surface),
        ("surfaceElevated", &tokens.colors.surface_elevated),
        ("foreground", &tokens.colors.foreground),
        ("secondaryForeground", &tokens.colors.secondary_foreground),
        ("accent", &tokens.colors.accent),
        ("success", &tokens.colors.success),
        ("warning", &tokens.colors.warning),
        ("danger", &tokens.colors.danger),
        ("shadow", &tokens.colors.shadow),
    ] {
        if color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!("colors.{name} must be #RRGGBB").into());
        }
    }

    for (name, value) in [
        ("typography.label", tokens.typography.label),
        ("typography.body", tokens.typography.body),
        ("typography.title", tokens.typography.title),
        ("typography.value", tokens.typography.value),
        ("spacing.small", tokens.spacing.small),
        ("spacing.medium", tokens.spacing.medium),
        ("spacing.large", tokens.spacing.large),
        ("radius.card", tokens.radius.card),
        ("radius.panel", tokens.radius.panel),
        ("layout.windowWidth", tokens.layout.window_width),
        ("layout.windowHeight", tokens.layout.window_height),
        ("layout.minWidgetWidth", tokens.layout.min_widget_width),
        ("layout.minWidgetHeight", tokens.layout.min_widget_height),
        ("layout.textWidgetHeight", tokens.layout.text_widget_height),
        (
            "layout.chartWidgetHeight",
            tokens.layout.chart_widget_height,
        ),
        (
            "layout.emptySectionHeight",
            tokens.layout.empty_section_height,
        ),
        ("layout.listBaseHeight", tokens.layout.list_base_height),
        ("layout.listLineHeight", tokens.layout.list_line_height),
        (
            "layout.sectionHeaderHeight",
            tokens.layout.section_header_height,
        ),
        ("layout.headerHeight", tokens.layout.header_height),
        ("layout.footerHeight", tokens.layout.footer_height),
        (
            "layout.statusIndicatorSize",
            tokens.layout.status_indicator_size,
        ),
        ("layout.gaugeTrackHeight", tokens.layout.gauge_track_height),
        (
            "layout.progressTrackHeight",
            tokens.layout.progress_track_height,
        ),
        ("layout.headingGap", tokens.layout.heading_gap),
        (
            "layout.chartBarBorderWidth",
            tokens.layout.chart_bar_border_width,
        ),
        ("layout.indicatorWidth", tokens.layout.indicator_width),
        ("layout.indicatorHeight", tokens.layout.indicator_height),
        ("layout.chromeHeight", tokens.layout.chrome_height),
        ("breakpoints.medium", tokens.breakpoints.medium),
        ("breakpoints.large", tokens.breakpoints.large),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(format!("{name} must be finite and positive").into());
        }
    }
    for (name, weight) in [
        ("typography.regularWeight", tokens.typography.regular_weight),
        (
            "typography.emphasisWeight",
            tokens.typography.emphasis_weight,
        ),
    ] {
        if !(1..=1_000).contains(&weight) {
            return Err(format!("{name} must be between 1 and 1000").into());
        }
    }
    if tokens.typography.regular_weight >= tokens.typography.emphasis_weight {
        return Err("typography.regularWeight must be below typography.emphasisWeight".into());
    }
    if !tokens.elevation.card_shadow_blur.is_finite()
        || tokens.elevation.card_shadow_blur < 0.0
        || !tokens.elevation.card_shadow_opacity.is_finite()
        || !(0.0..=1.0).contains(&tokens.elevation.card_shadow_opacity)
    {
        return Err("elevation values are invalid".into());
    }
    if tokens.breakpoints.medium >= tokens.breakpoints.large {
        return Err("breakpoints.medium must be below breakpoints.large".into());
    }
    if tokens.motion.fast_milliseconds == 0 || tokens.motion.page_milliseconds == 0 {
        return Err("motion durations must be positive".into());
    }
    for (name, value) in [
        ("layout.textMaxLines", tokens.layout.text_max_lines),
        ("layout.maxTextChars", tokens.layout.max_text_chars),
        ("layout.maxListItems", tokens.layout.max_list_items),
        ("layout.maxChartSamples", tokens.layout.max_chart_samples),
        (
            "layout.maxVisibleIndicators",
            tokens.layout.max_visible_indicators,
        ),
    ] {
        if value == 0 {
            return Err(format!("{name} must be positive").into());
        }
    }
    Ok(())
}

fn slint_source(tokens: &TokenFile) -> String {
    let colors = &tokens.colors;
    let mut output = String::new();
    writeln!(
        output,
        "// Generated from themes/default/tokens.json. Do not edit."
    )
    .unwrap();
    writeln!(output, "export global Theme {{").unwrap();
    for (name, value) in [
        ("background", &colors.background),
        ("surface", &colors.surface),
        ("surface-elevated", &colors.surface_elevated),
        ("foreground", &colors.foreground),
        ("secondary-foreground", &colors.secondary_foreground),
        ("accent", &colors.accent),
        ("success", &colors.success),
        ("warning", &colors.warning),
        ("danger", &colors.danger),
        ("shadow", &colors.shadow),
    ] {
        writeln!(output, "    out property <color> {name}: {value};").unwrap();
    }
    for (name, value) in [
        ("font-label", tokens.typography.label),
        ("font-body", tokens.typography.body),
        ("font-title", tokens.typography.title),
        ("font-value", tokens.typography.value),
        ("spacing-small", tokens.spacing.small),
        ("spacing-medium", tokens.spacing.medium),
        ("spacing-large", tokens.spacing.large),
        ("radius-card", tokens.radius.card),
        ("radius-panel", tokens.radius.panel),
        ("shadow-blur", tokens.elevation.card_shadow_blur),
        ("breakpoint-medium", tokens.breakpoints.medium),
        ("breakpoint-large", tokens.breakpoints.large),
        ("window-width", tokens.layout.window_width),
        ("window-height", tokens.layout.window_height),
        ("min-widget-width", tokens.layout.min_widget_width),
        ("min-widget-height", tokens.layout.min_widget_height),
        ("text-widget-height", tokens.layout.text_widget_height),
        ("chart-widget-height", tokens.layout.chart_widget_height),
        ("empty-section-height", tokens.layout.empty_section_height),
        ("list-base-height", tokens.layout.list_base_height),
        ("list-line-height", tokens.layout.list_line_height),
        ("section-header-height", tokens.layout.section_header_height),
        ("header-height", tokens.layout.header_height),
        ("footer-height", tokens.layout.footer_height),
        ("status-indicator-size", tokens.layout.status_indicator_size),
        ("gauge-track-height", tokens.layout.gauge_track_height),
        ("progress-track-height", tokens.layout.progress_track_height),
        ("heading-gap", tokens.layout.heading_gap),
        (
            "chart-bar-border-width",
            tokens.layout.chart_bar_border_width,
        ),
        ("indicator-width", tokens.layout.indicator_width),
        ("indicator-height", tokens.layout.indicator_height),
        ("chrome-height", tokens.layout.chrome_height),
    ] {
        writeln!(output, "    out property <length> {name}: {value}px;").unwrap();
    }
    for (name, value) in [
        (
            "font-regular-weight",
            usize::from(tokens.typography.regular_weight),
        ),
        (
            "font-emphasis-weight",
            usize::from(tokens.typography.emphasis_weight),
        ),
        ("text-max-lines", tokens.layout.text_max_lines),
    ] {
        writeln!(output, "    out property <int> {name}: {value};").unwrap();
    }
    writeln!(
        output,
        "    out property <float> shadow-opacity: {};",
        tokens.elevation.card_shadow_opacity
    )
    .unwrap();
    writeln!(
        output,
        "    out property <duration> motion-fast: {}ms;",
        tokens.motion.fast_milliseconds
    )
    .unwrap();
    writeln!(
        output,
        "    out property <duration> motion-page: {}ms;",
        tokens.motion.page_milliseconds
    )
    .unwrap();
    writeln!(output, "}}").unwrap();
    output
}

fn rust_source(tokens: &TokenFile) -> String {
    let colors = &tokens.colors;
    format!(
        r#"// Generated from themes/default/tokens.json. Do not edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorTokens {{
    pub background: &'static str,
    pub surface: &'static str,
    pub surface_elevated: &'static str,
    pub foreground: &'static str,
    pub secondary_foreground: &'static str,
    pub accent: &'static str,
    pub success: &'static str,
    pub warning: &'static str,
    pub danger: &'static str,
    pub shadow: &'static str,
}}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesignTokens {{
    pub colors: ColorTokens,
    pub font_label: f32,
    pub font_body: f32,
    pub font_title: f32,
    pub font_value: f32,
    pub font_regular_weight: u16,
    pub font_emphasis_weight: u16,
    pub spacing_small: f32,
    pub spacing_medium: f32,
    pub spacing_large: f32,
    pub radius_card: f32,
    pub radius_panel: f32,
    pub shadow_blur: f32,
    pub shadow_opacity: f32,
    pub motion_fast_ms: u32,
    pub motion_page_ms: u32,
    pub breakpoint_medium: f32,
    pub breakpoint_large: f32,
    pub window_width: f32,
    pub window_height: f32,
    pub min_widget_width: f32,
    pub min_widget_height: f32,
    pub text_widget_height: f32,
    pub chart_widget_height: f32,
    pub empty_section_height: f32,
    pub list_base_height: f32,
    pub list_line_height: f32,
    pub section_header_height: f32,
    pub header_height: f32,
    pub footer_height: f32,
    pub status_indicator_size: f32,
    pub gauge_track_height: f32,
    pub progress_track_height: f32,
    pub heading_gap: f32,
    pub chart_bar_border_width: f32,
    pub indicator_width: f32,
    pub indicator_height: f32,
    pub chrome_height: f32,
    pub text_max_lines: usize,
    pub max_text_chars: usize,
    pub max_list_items: usize,
    pub max_chart_samples: usize,
    pub max_visible_indicators: usize,
}}

pub const DEFAULT: DesignTokens = DesignTokens {{
    colors: ColorTokens {{
        background: "{}",
        surface: "{}",
        surface_elevated: "{}",
        foreground: "{}",
        secondary_foreground: "{}",
        accent: "{}",
        success: "{}",
        warning: "{}",
        danger: "{}",
        shadow: "{}",
    }},
    font_label: {}f32,
    font_body: {}f32,
    font_title: {}f32,
    font_value: {}f32,
    font_regular_weight: {},
    font_emphasis_weight: {},
    spacing_small: {}f32,
    spacing_medium: {}f32,
    spacing_large: {}f32,
    radius_card: {}f32,
    radius_panel: {}f32,
    shadow_blur: {}f32,
    shadow_opacity: {}f32,
    motion_fast_ms: {},
    motion_page_ms: {},
    breakpoint_medium: {}f32,
    breakpoint_large: {}f32,
    window_width: {}f32,
    window_height: {}f32,
    min_widget_width: {}f32,
    min_widget_height: {}f32,
    text_widget_height: {}f32,
    chart_widget_height: {}f32,
    empty_section_height: {}f32,
    list_base_height: {}f32,
    list_line_height: {}f32,
    section_header_height: {}f32,
    header_height: {}f32,
    footer_height: {}f32,
    status_indicator_size: {}f32,
    gauge_track_height: {}f32,
    progress_track_height: {}f32,
    heading_gap: {}f32,
    chart_bar_border_width: {}f32,
    indicator_width: {}f32,
    indicator_height: {}f32,
    chrome_height: {}f32,
    text_max_lines: {},
    max_text_chars: {},
    max_list_items: {},
    max_chart_samples: {},
    max_visible_indicators: {},
}};
"#,
        colors.background,
        colors.surface,
        colors.surface_elevated,
        colors.foreground,
        colors.secondary_foreground,
        colors.accent,
        colors.success,
        colors.warning,
        colors.danger,
        colors.shadow,
        tokens.typography.label,
        tokens.typography.body,
        tokens.typography.title,
        tokens.typography.value,
        tokens.typography.regular_weight,
        tokens.typography.emphasis_weight,
        tokens.spacing.small,
        tokens.spacing.medium,
        tokens.spacing.large,
        tokens.radius.card,
        tokens.radius.panel,
        tokens.elevation.card_shadow_blur,
        tokens.elevation.card_shadow_opacity,
        tokens.motion.fast_milliseconds,
        tokens.motion.page_milliseconds,
        tokens.breakpoints.medium,
        tokens.breakpoints.large,
        tokens.layout.window_width,
        tokens.layout.window_height,
        tokens.layout.min_widget_width,
        tokens.layout.min_widget_height,
        tokens.layout.text_widget_height,
        tokens.layout.chart_widget_height,
        tokens.layout.empty_section_height,
        tokens.layout.list_base_height,
        tokens.layout.list_line_height,
        tokens.layout.section_header_height,
        tokens.layout.header_height,
        tokens.layout.footer_height,
        tokens.layout.status_indicator_size,
        tokens.layout.gauge_track_height,
        tokens.layout.progress_track_height,
        tokens.layout.heading_gap,
        tokens.layout.chart_bar_border_width,
        tokens.layout.indicator_width,
        tokens.layout.indicator_height,
        tokens.layout.chrome_height,
        tokens.layout.text_max_lines,
        tokens.layout.max_text_chars,
        tokens.layout.max_list_items,
        tokens.layout.max_chart_samples,
        tokens.layout.max_visible_indicators,
    )
}
