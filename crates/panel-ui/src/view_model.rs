use std::{error::Error, fmt};

use panel_model::{ModelError, PageModel, StatusTone, Widget};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutMetrics {
    pub content_padding: f32,
    pub column_gap: f32,
    pub row_gap: f32,
    pub min_widget_width: f32,
    pub min_widget_height: f32,
    pub section_header_height: f32,
    pub chrome_height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewOptions {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub scale: f32,
    pub reduce_motion: bool,
    pub max_text_chars: usize,
    pub max_list_items: usize,
    pub layout: LayoutMetrics,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageView {
    pub id: String,
    pub title: String,
    pub icon: Option<String>,
    pub sections: Vec<SectionView>,
    pub layout: PageLayout,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SectionView {
    pub title: Option<String>,
    pub widgets: Vec<WidgetView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageLayout {
    pub column_count: usize,
    pub vertical_overflow: bool,
    pub reduce_motion: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticTone {
    Neutral,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetView {
    Metric {
        label: String,
        value: String,
        unit: Option<String>,
    },
    Status {
        label: String,
        value: String,
        tone: SemanticTone,
    },
    Gauge {
        label: String,
        value: f32,
    },
    Text {
        text: String,
    },
    Progress {
        label: String,
        value: f32,
    },
    List {
        items: Vec<String>,
        hidden_item_count: usize,
    },
    Chart {
        label: String,
        normalized_points: Vec<f32>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageIndicatorWindow {
    pub visible_indices: Vec<usize>,
    pub selected_index: usize,
    pub hidden_before: usize,
    pub hidden_after: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ViewModelError {
    InvalidModel(ModelError),
    InvalidDimension(&'static str),
    InvalidNonNegativeDimension(&'static str),
    InvalidLimit(&'static str),
    InvalidIcon,
    InvalidSelectedPage {
        selected_index: usize,
        page_count: usize,
    },
}

impl From<ModelError> for ViewModelError {
    fn from(error: ModelError) -> Self {
        Self::InvalidModel(error)
    }
}

impl fmt::Display for ViewModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModel(error) => write!(formatter, "page model is invalid: {error}"),
            Self::InvalidDimension(name) => {
                write!(
                    formatter,
                    "view option {name} must be finite and greater than zero"
                )
            }
            Self::InvalidNonNegativeDimension(name) => {
                write!(
                    formatter,
                    "view option {name} must be finite and non-negative"
                )
            }
            Self::InvalidLimit(name) => {
                write!(formatter, "view option {name} must be greater than zero")
            }
            Self::InvalidIcon => formatter.write_str("page icon must be a semantic identifier"),
            Self::InvalidSelectedPage {
                selected_index,
                page_count,
            } => write!(
                formatter,
                "selected page index {selected_index} is outside {page_count} pages"
            ),
        }
    }
}

impl Error for ViewModelError {}

impl PageView {
    pub fn from_model(model: &PageModel, options: ViewOptions) -> Result<Self, ViewModelError> {
        let icon = model
            .icon
            .as_deref()
            .map(validate_semantic_icon)
            .transpose()?;
        model.validate()?;
        validate_options(options)?;

        let column_count = adaptive_column_count(options);
        let sections = model
            .sections
            .iter()
            .map(|section| SectionView {
                title: section
                    .title
                    .as_deref()
                    .map(|title| truncate_text(title, options.max_text_chars)),
                widgets: section
                    .widgets
                    .iter()
                    .map(|widget| widget_view(widget, options))
                    .collect(),
            })
            .collect::<Vec<_>>();
        let vertical_overflow =
            estimate_content_height(&sections, column_count, options) > logical_height(options);

        Ok(Self {
            id: model.id.as_str().to_owned(),
            title: truncate_text(&model.title, options.max_text_chars),
            icon,
            sections,
            layout: PageLayout {
                column_count,
                vertical_overflow,
                reduce_motion: options.reduce_motion,
            },
        })
    }
}

pub fn page_indicator_window(
    page_count: usize,
    selected_index: usize,
    max_visible: usize,
) -> Result<PageIndicatorWindow, ViewModelError> {
    if max_visible == 0 {
        return Err(ViewModelError::InvalidLimit("max_visible"));
    }
    if selected_index >= page_count {
        return Err(ViewModelError::InvalidSelectedPage {
            selected_index,
            page_count,
        });
    }

    let visible_count = page_count.min(max_visible);
    let half = visible_count / 2;
    let start = selected_index
        .saturating_sub(half)
        .min(page_count - visible_count);
    let end = start + visible_count;

    Ok(PageIndicatorWindow {
        visible_indices: (start..end).collect(),
        selected_index,
        hidden_before: start,
        hidden_after: page_count - end,
    })
}

fn validate_options(options: ViewOptions) -> Result<(), ViewModelError> {
    for (name, value) in [
        ("viewport_width", options.viewport_width),
        ("viewport_height", options.viewport_height),
        ("scale", options.scale),
        ("min_widget_width", options.layout.min_widget_width),
        ("min_widget_height", options.layout.min_widget_height),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(ViewModelError::InvalidDimension(name));
        }
    }
    for (name, value) in [
        ("content_padding", options.layout.content_padding),
        ("column_gap", options.layout.column_gap),
        ("row_gap", options.layout.row_gap),
        (
            "section_header_height",
            options.layout.section_header_height,
        ),
        ("chrome_height", options.layout.chrome_height),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(ViewModelError::InvalidNonNegativeDimension(name));
        }
    }
    if options.max_text_chars == 0 {
        return Err(ViewModelError::InvalidLimit("max_text_chars"));
    }
    if options.max_list_items == 0 {
        return Err(ViewModelError::InvalidLimit("max_list_items"));
    }
    Ok(())
}

fn validate_semantic_icon(icon: &str) -> Result<String, ViewModelError> {
    let is_identifier = !icon.is_empty()
        && icon
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if is_identifier {
        Ok(icon.to_owned())
    } else {
        Err(ViewModelError::InvalidIcon)
    }
}

fn adaptive_column_count(options: ViewOptions) -> usize {
    let logical_width = f64::from(options.viewport_width) / f64::from(options.scale);
    let content_padding = f64::from(options.layout.content_padding);
    let column_gap = f64::from(options.layout.column_gap);
    let min_widget_width = f64::from(options.layout.min_widget_width);
    let content_width = (logical_width - content_padding * 2.0).max(0.0);
    let columns = ((content_width + column_gap) / (min_widget_width + column_gap)).floor() as usize;
    columns.clamp(1, 3)
}

fn logical_height(options: ViewOptions) -> f64 {
    (f64::from(options.viewport_height) / f64::from(options.scale)
        - f64::from(options.layout.chrome_height))
    .max(0.0)
}

fn estimate_content_height(
    sections: &[SectionView],
    column_count: usize,
    options: ViewOptions,
) -> f64 {
    let row_gap = f64::from(options.layout.row_gap);
    let mut height = f64::from(options.layout.content_padding) * 2.0;
    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            height += row_gap;
        }
        if section.title.is_some() {
            height += f64::from(options.layout.section_header_height) + row_gap;
        }
        let rows = section.widgets.len().div_ceil(column_count);
        if rows > 0 {
            height += rows as f64 * f64::from(options.layout.min_widget_height);
            height += rows.saturating_sub(1) as f64 * row_gap;
        }
    }
    height
}

fn widget_view(widget: &Widget, options: ViewOptions) -> WidgetView {
    let text = |value: &str| truncate_text(value, options.max_text_chars);
    match widget {
        Widget::Metric(value) => WidgetView::Metric {
            label: text(&value.label),
            value: text(&value.value),
            unit: value.unit.as_deref().map(text),
        },
        Widget::Status(value) => WidgetView::Status {
            label: text(&value.label),
            value: text(&value.value),
            tone: match value.tone {
                StatusTone::Neutral => SemanticTone::Neutral,
                StatusTone::Success => SemanticTone::Success,
                StatusTone::Warning => SemanticTone::Warning,
                StatusTone::Danger => SemanticTone::Danger,
            },
        },
        Widget::Gauge(value) => WidgetView::Gauge {
            label: text(&value.label),
            value: value.value as f32,
        },
        Widget::Text(value) => WidgetView::Text {
            text: text(&value.text),
        },
        Widget::Progress(value) => WidgetView::Progress {
            label: text(&value.label),
            value: value.value as f32,
        },
        Widget::List(value) => {
            let visible_count = value.items.len().min(options.max_list_items);
            WidgetView::List {
                items: value.items[..visible_count]
                    .iter()
                    .map(|item| text(item))
                    .collect(),
                hidden_item_count: value.items.len() - visible_count,
            }
        }
        Widget::Chart(value) => WidgetView::Chart {
            label: text(&value.label),
            normalized_points: normalize_chart_points(&value.points),
        },
    }
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    if max_chars == 1 {
        return "…".to_owned();
    }

    let mut truncated = value.chars().take(max_chars - 1).collect::<String>();
    truncated.push('…');
    truncated
}

fn normalize_chart_points(points: &[f64]) -> Vec<f32> {
    let Some(maximum_magnitude) = points.iter().map(|point| point.abs()).reduce(f64::max) else {
        return Vec::new();
    };
    if maximum_magnitude == 0.0 {
        return vec![0.5; points.len()];
    }

    let scaled = points
        .iter()
        .map(|point| point / maximum_magnitude)
        .collect::<Vec<_>>();
    let minimum = scaled.iter().copied().reduce(f64::min).unwrap_or(0.0);
    let maximum = scaled.iter().copied().reduce(f64::max).unwrap_or(0.0);
    if minimum == maximum {
        return vec![0.5; points.len()];
    }

    scaled
        .into_iter()
        .map(|point| ((point - minimum) / (maximum - minimum)) as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use panel_model::{
        ChartWidget, GaugeWidget, ListWidget, MAX_CHART_POINTS, MAX_SECTIONS, MAX_WIDGETS,
        MetricWidget, PageId, ProgressWidget, Section, StatusWidget, TextWidget,
    };

    use super::*;

    fn options(width: f32, height: f32) -> ViewOptions {
        ViewOptions {
            viewport_width: width,
            viewport_height: height,
            scale: 1.0,
            reduce_motion: false,
            max_text_chars: 24,
            max_list_items: 3,
            layout: LayoutMetrics {
                content_padding: 24.0,
                column_gap: 14.0,
                row_gap: 14.0,
                min_widget_width: 220.0,
                min_widget_height: 120.0,
                section_header_height: 28.0,
                chrome_height: 96.0,
            },
        }
    }

    fn page(sections: Vec<Section>) -> PageModel {
        PageModel {
            schema_version: 1,
            id: PageId::new("plugin.page").unwrap(),
            title: "Plugin page".into(),
            icon: Some("server-status".into()),
            sections,
            updated_at_unix_ms: None,
        }
    }

    #[test]
    fn converts_every_widget_variant_to_ui_safe_values() {
        let model = page(vec![Section {
            title: Some("Status".into()),
            widgets: vec![
                Widget::Metric(MetricWidget {
                    label: "CPU".into(),
                    value: "42".into(),
                    unit: Some("%".into()),
                }),
                Widget::Status(StatusWidget {
                    label: "API".into(),
                    value: "Online".into(),
                    tone: StatusTone::Success,
                }),
                Widget::Gauge(GaugeWidget {
                    label: "Memory".into(),
                    value: 0.75,
                }),
                Widget::Text(TextWidget {
                    text: "Long-form status".into(),
                }),
                Widget::Progress(ProgressWidget {
                    label: "Build".into(),
                    value: 0.5,
                }),
                Widget::List(ListWidget {
                    items: vec!["one".into(), "two".into(), "three".into(), "four".into()],
                }),
                Widget::Chart(ChartWidget {
                    label: "Load".into(),
                    points: vec![-10.0, 0.0, 10.0],
                }),
            ],
        }]);

        let view = PageView::from_model(&model, options(900.0, 900.0)).unwrap();

        assert_eq!(view.sections[0].widgets.len(), 7);
        assert!(matches!(
            &view.sections[0].widgets[1],
            WidgetView::Status {
                tone: SemanticTone::Success,
                ..
            }
        ));
        assert_eq!(
            view.sections[0].widgets[5],
            WidgetView::List {
                items: vec!["one".into(), "two".into(), "three".into()],
                hidden_item_count: 1,
            }
        );
        assert_eq!(
            view.sections[0].widgets[6],
            WidgetView::Chart {
                label: "Load".into(),
                normalized_points: vec![0.0, 0.5, 1.0],
            }
        );
    }

    #[test]
    fn adapts_columns_for_width_and_scale() {
        assert_eq!(
            PageView::from_model(&page(Vec::new()), options(480.0, 540.0))
                .unwrap()
                .layout
                .column_count,
            1
        );
        assert_eq!(
            PageView::from_model(&page(Vec::new()), options(720.0, 540.0))
                .unwrap()
                .layout
                .column_count,
            2
        );
        assert_eq!(
            PageView::from_model(&page(Vec::new()), options(900.0, 540.0))
                .unwrap()
                .layout
                .column_count,
            3
        );

        let mut scaled = options(900.0, 540.0);
        scaled.scale = 2.0;
        assert_eq!(
            PageView::from_model(&page(Vec::new()), scaled)
                .unwrap()
                .layout
                .column_count,
            1
        );
    }

    #[test]
    fn preserves_empty_sections_and_detects_vertical_overflow() {
        let model = page(vec![
            Section {
                title: Some("Empty".into()),
                widgets: Vec::new(),
            },
            Section {
                title: None,
                widgets: (0..8)
                    .map(|index| {
                        Widget::Text(TextWidget {
                            text: format!("item {index}"),
                        })
                    })
                    .collect(),
            },
        ]);

        let view = PageView::from_model(&model, options(480.0, 300.0)).unwrap();

        assert!(view.sections[0].widgets.is_empty());
        assert!(view.layout.vertical_overflow);
        assert_eq!(view.sections[1].widgets.len(), 8);
    }

    #[test]
    fn truncates_unicode_text_without_splitting_characters() {
        let model = page(vec![Section {
            title: None,
            widgets: vec![Widget::Text(TextWidget {
                text: "状态正常，所有系统运行稳定".into(),
            })],
        }]);
        let mut compact = options(480.0, 540.0);
        compact.max_text_chars = 6;

        let view = PageView::from_model(&model, compact).unwrap();

        assert_eq!(
            view.sections[0].widgets[0],
            WidgetView::Text {
                text: "状态正常，…".into()
            }
        );
    }

    #[test]
    fn handles_maximum_size_valid_model() {
        let mut sections = (0..MAX_SECTIONS)
            .map(|_| Section {
                title: None,
                widgets: Vec::new(),
            })
            .collect::<Vec<_>>();
        for index in 0..MAX_WIDGETS {
            sections[index % MAX_SECTIONS]
                .widgets
                .push(Widget::Text(TextWidget {
                    text: format!("widget {index}"),
                }));
        }
        sections[0].widgets[0] = Widget::Chart(ChartWidget {
            label: "Maximum chart".into(),
            points: vec![f64::MAX; MAX_CHART_POINTS],
        });
        let model = page(sections);

        let view = PageView::from_model(&model, options(900.0, 540.0)).unwrap();

        assert_eq!(
            view.sections
                .iter()
                .map(|section| section.widgets.len())
                .sum::<usize>(),
            MAX_WIDGETS
        );
        let WidgetView::Chart {
            normalized_points, ..
        } = &view.sections[0].widgets[0]
        else {
            panic!("first widget should remain a chart");
        };
        assert_eq!(normalized_points.len(), MAX_CHART_POINTS);
        assert!(normalized_points.iter().all(|point| point.is_finite()));
    }

    #[test]
    fn reduce_motion_is_carried_into_the_view_contract() {
        let mut reduced = options(900.0, 540.0);
        reduced.reduce_motion = true;

        let view = PageView::from_model(&page(Vec::new()), reduced).unwrap();

        assert!(view.layout.reduce_motion);
    }

    #[test]
    fn rejects_non_positive_or_non_finite_required_dimensions() {
        for field in [
            "viewport_width",
            "viewport_height",
            "scale",
            "min_widget_width",
            "min_widget_height",
        ] {
            for value in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = options(900.0, 540.0);
                match field {
                    "viewport_width" => invalid.viewport_width = value,
                    "viewport_height" => invalid.viewport_height = value,
                    "scale" => invalid.scale = value,
                    "min_widget_width" => invalid.layout.min_widget_width = value,
                    "min_widget_height" => invalid.layout.min_widget_height = value,
                    _ => unreachable!(),
                }

                assert_eq!(
                    PageView::from_model(&page(Vec::new()), invalid),
                    Err(ViewModelError::InvalidDimension(field)),
                    "accepted {field}={value:?}"
                );
            }
        }
    }

    #[test]
    fn rejects_negative_or_non_finite_non_negative_dimensions() {
        for field in [
            "content_padding",
            "column_gap",
            "row_gap",
            "section_header_height",
            "chrome_height",
        ] {
            for value in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut invalid = options(900.0, 540.0);
                match field {
                    "content_padding" => invalid.layout.content_padding = value,
                    "column_gap" => invalid.layout.column_gap = value,
                    "row_gap" => invalid.layout.row_gap = value,
                    "section_header_height" => invalid.layout.section_header_height = value,
                    "chrome_height" => invalid.layout.chrome_height = value,
                    _ => unreachable!(),
                }

                assert_eq!(
                    PageView::from_model(&page(Vec::new()), invalid),
                    Err(ViewModelError::InvalidNonNegativeDimension(field)),
                    "accepted {field}={value:?}"
                );
            }
        }
    }

    #[test]
    fn extreme_finite_dimensions_do_not_overflow_layout_arithmetic() {
        let mut extreme = options(f32::MAX, f32::MAX);
        extreme.scale = f32::MIN_POSITIVE;
        extreme.layout.content_padding = f32::MAX;
        extreme.layout.column_gap = f32::MAX;
        extreme.layout.row_gap = f32::MAX;
        extreme.layout.min_widget_width = f32::MIN_POSITIVE;
        extreme.layout.min_widget_height = f32::MAX;
        extreme.layout.section_header_height = f32::MAX;
        extreme.layout.chrome_height = f32::MAX;

        let view = PageView::from_model(&page(Vec::new()), extreme).unwrap();

        assert_eq!(view.layout.column_count, 3);
        assert!(!view.layout.vertical_overflow);
    }

    #[test]
    fn accepts_zero_for_non_negative_layout_dimensions() {
        let mut zeroed = options(900.0, 540.0);
        zeroed.layout.content_padding = 0.0;
        zeroed.layout.column_gap = 0.0;
        zeroed.layout.row_gap = 0.0;
        zeroed.layout.section_header_height = 0.0;
        zeroed.layout.chrome_height = 0.0;

        let view = PageView::from_model(&page(Vec::new()), zeroed).unwrap();

        assert_eq!(view.layout.column_count, 3);
        assert!(!view.layout.vertical_overflow);
    }

    #[test]
    fn folds_many_page_indicators_around_selection() {
        let indicators = page_indicator_window(100, 50, 7).unwrap();

        assert_eq!(indicators.visible_indices, (47..54).collect::<Vec<_>>());
        assert_eq!(indicators.hidden_before, 47);
        assert_eq!(indicators.hidden_after, 46);
    }

    #[test]
    fn indicator_window_stays_inside_edges() {
        assert_eq!(
            page_indicator_window(10, 0, 5).unwrap().visible_indices,
            (0..5).collect::<Vec<_>>()
        );
        assert_eq!(
            page_indicator_window(10, 9, 5).unwrap().visible_indices,
            (5..10).collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_path_like_icons_before_they_reach_slint() {
        let mut model = page(Vec::new());
        model.icon = Some("../../font.ttf".into());

        assert_eq!(
            PageView::from_model(&model, options(900.0, 540.0)),
            Err(ViewModelError::InvalidIcon)
        );
    }
}
