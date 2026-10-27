use panel_model::PageModel;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use thiserror::Error;

use crate::design_tokens::DEFAULT;
use crate::view_model::{
    LayoutMetrics, PageView, SemanticTone, ViewModelError, ViewOptions, WidgetView,
    page_indicator_window,
};
use crate::{AppWindow, IndicatorItem, WidgetBlock};

const SECTION_KIND: i32 = 0;
const METRIC_KIND: i32 = 1;
const STATUS_KIND: i32 = 2;
const GAUGE_KIND: i32 = 3;
const TEXT_KIND: i32 = 4;
const PROGRESS_KIND: i32 = 5;
const LIST_KIND: i32 = 6;
const CHART_KIND: i32 = 7;
const EMPTY_SECTION_KIND: i32 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RendererConfig {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub scale: f32,
    pub reduce_motion: bool,
    pub page_count: usize,
    pub selected_page: usize,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            viewport_width: 900.0,
            viewport_height: 540.0,
            scale: 1.0,
            reduce_motion: false,
            page_count: 1,
            selected_page: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderBlock {
    pub kind: i32,
    pub label: String,
    pub value: String,
    pub unit: String,
    pub detail: String,
    pub points: Vec<f32>,
    pub tone: i32,
    pub fraction: f32,
    pub column: usize,
    pub top: f32,
    pub card_height: f32,
    pub full_span: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResponsiveBlocks {
    pub columns: usize,
    pub blocks: Vec<RenderBlock>,
    pub content_height: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderIndicator {
    pub page_index: usize,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPayload {
    pub title: String,
    pub icon: String,
    pub scale: f32,
    pub reduce_motion: bool,
    pub layouts: [ResponsiveBlocks; 3],
    pub indicators: Vec<RenderIndicator>,
}

#[derive(Debug, Error)]
pub enum UiError {
    #[error(transparent)]
    View(#[from] ViewModelError),
    #[error(transparent)]
    Platform(#[from] slint::PlatformError),
}

pub fn build_payload(
    model: &PageModel,
    config: RendererConfig,
) -> Result<RenderPayload, ViewModelError> {
    let view = PageView::from_model(
        model,
        ViewOptions {
            viewport_width: config.viewport_width,
            viewport_height: config.viewport_height,
            scale: config.scale,
            reduce_motion: config.reduce_motion,
            max_text_chars: DEFAULT.max_text_chars,
            max_list_items: DEFAULT.max_list_items,
            layout: LayoutMetrics {
                content_padding: DEFAULT.spacing_large,
                column_gap: DEFAULT.spacing_medium,
                row_gap: DEFAULT.spacing_medium,
                min_widget_width: DEFAULT.min_widget_width,
                min_widget_height: DEFAULT.min_widget_height,
                section_header_height: DEFAULT.section_header_height,
                chrome_height: DEFAULT.chrome_height,
            },
        },
    )?;

    let indicator_window = page_indicator_window(
        config.page_count,
        config.selected_page,
        DEFAULT.max_visible_indicators,
    )?;
    let mut indicators = Vec::with_capacity(indicator_window.visible_indices.len() + 2);
    if indicator_window.hidden_before > 0 {
        indicators.push(RenderIndicator {
            page_index: indicator_window.visible_indices[0],
            label: "…".into(),
            selected: false,
        });
    }
    indicators.extend(
        indicator_window
            .visible_indices
            .iter()
            .copied()
            .map(|page_index| RenderIndicator {
                page_index,
                label: String::new(),
                selected: page_index == indicator_window.selected_index,
            }),
    );
    if indicator_window.hidden_after > 0 {
        indicators.push(RenderIndicator {
            page_index: *indicator_window.visible_indices.last().unwrap_or(&0),
            label: "…".into(),
            selected: false,
        });
    }

    let layouts = [
        layout_blocks(&view, 1),
        layout_blocks(&view, 2),
        layout_blocks(&view, 3),
    ];
    Ok(RenderPayload {
        title: view.title,
        icon: view.icon.unwrap_or_default(),
        scale: config.scale,
        reduce_motion: view.layout.reduce_motion,
        layouts,
        indicators,
    })
}

pub fn apply_payload(window: &AppWindow, payload: RenderPayload) {
    window.set_page_title(payload.title.into());
    window.set_page_icon(payload.icon.into());
    window.set_ui_scale(payload.scale);
    window.set_reduce_motion(payload.reduce_motion);

    let [one, two, three] = payload.layouts;
    window.set_content_height_one(one.content_height);
    window.set_content_height_two(two.content_height);
    window.set_content_height_three(three.content_height);
    window.set_blocks_one(model(one.blocks.into_iter().map(slint_block).collect()));
    window.set_blocks_two(model(two.blocks.into_iter().map(slint_block).collect()));
    window.set_blocks_three(model(three.blocks.into_iter().map(slint_block).collect()));
    window.set_indicators(model(
        payload
            .indicators
            .into_iter()
            .map(|indicator| IndicatorItem {
                label: indicator.label.into(),
                selected: indicator.selected,
                page_index: indicator.page_index as i32,
            })
            .collect(),
    ));
}

pub fn set_page(
    window: &AppWindow,
    model: &PageModel,
    config: RendererConfig,
) -> Result<(), ViewModelError> {
    apply_payload(window, build_payload(model, config)?);
    Ok(())
}

pub fn run_page(model: &PageModel, config: RendererConfig) -> Result<(), UiError> {
    let window = AppWindow::new()?;
    set_page(&window, model, config)?;
    window.run()?;
    Ok(())
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn slint_block(block: RenderBlock) -> WidgetBlock {
    WidgetBlock {
        kind: block.kind,
        label: SharedString::from(block.label),
        value: SharedString::from(block.value),
        unit: SharedString::from(block.unit),
        detail: SharedString::from(block.detail),
        points: model(block.points),
        tone: block.tone,
        fraction: block.fraction,
        column: block.column as i32,
        top: block.top,
        card_height: block.card_height,
        full_span: block.full_span,
    }
}

fn layout_blocks(view: &PageView, columns: usize) -> ResponsiveBlocks {
    let mut blocks = Vec::new();
    let mut top = DEFAULT.spacing_small;

    for section in &view.sections {
        if let Some(title) = &section.title {
            blocks.push(RenderBlock {
                kind: SECTION_KIND,
                label: title.clone(),
                value: String::new(),
                unit: String::new(),
                detail: String::new(),
                points: Vec::new(),
                tone: 0,
                fraction: 0.0,
                column: 0,
                top,
                card_height: DEFAULT.section_header_height,
                full_span: true,
            });
            top += DEFAULT.section_header_height + DEFAULT.spacing_small;
        }

        if section.widgets.is_empty() {
            blocks.push(RenderBlock {
                kind: EMPTY_SECTION_KIND,
                label: String::new(),
                value: String::new(),
                unit: String::new(),
                detail: String::new(),
                points: Vec::new(),
                tone: 0,
                fraction: 0.0,
                column: 0,
                top,
                card_height: DEFAULT.empty_section_height,
                full_span: true,
            });
            top += DEFAULT.empty_section_height + DEFAULT.spacing_medium;
            continue;
        }

        for row in section.widgets.chunks(columns) {
            let row_height = row
                .iter()
                .map(widget_height)
                .fold(DEFAULT.min_widget_height, f32::max);
            for (column, widget) in row.iter().enumerate() {
                blocks.push(widget_block(widget, column, top, row_height));
            }
            top += row_height + DEFAULT.spacing_medium;
        }
    }

    ResponsiveBlocks {
        columns,
        blocks,
        content_height: top + DEFAULT.spacing_small,
    }
}

fn widget_height(widget: &WidgetView) -> f32 {
    match widget {
        WidgetView::Text { .. } => DEFAULT.min_widget_height.max(DEFAULT.text_widget_height),
        WidgetView::List { items, .. } => (DEFAULT.list_base_height
            + items.len() as f32 * DEFAULT.list_line_height)
            .max(DEFAULT.min_widget_height),
        WidgetView::Chart { .. } => DEFAULT.min_widget_height.max(DEFAULT.chart_widget_height),
        WidgetView::Metric { .. }
        | WidgetView::Status { .. }
        | WidgetView::Gauge { .. }
        | WidgetView::Progress { .. } => DEFAULT.min_widget_height,
    }
}

fn widget_block(widget: &WidgetView, column: usize, top: f32, card_height: f32) -> RenderBlock {
    let mut block = RenderBlock {
        kind: TEXT_KIND,
        label: String::new(),
        value: String::new(),
        unit: String::new(),
        detail: String::new(),
        points: Vec::new(),
        tone: 0,
        fraction: 0.0,
        column,
        top,
        card_height,
        full_span: false,
    };

    match widget {
        WidgetView::Metric { label, value, unit } => {
            block.kind = METRIC_KIND;
            block.label.clone_from(label);
            block.value.clone_from(value);
            block.unit = unit.clone().unwrap_or_default();
        }
        WidgetView::Status { label, value, tone } => {
            block.kind = STATUS_KIND;
            block.label.clone_from(label);
            block.value.clone_from(value);
            block.tone = tone_value(*tone);
        }
        WidgetView::Gauge { label, value } => {
            block.kind = GAUGE_KIND;
            block.label.clone_from(label);
            block.value = percentage(*value);
            block.fraction = *value;
        }
        WidgetView::Text { text } => {
            block.kind = TEXT_KIND;
            block.detail.clone_from(text);
        }
        WidgetView::Progress { label, value } => {
            block.kind = PROGRESS_KIND;
            block.label.clone_from(label);
            block.value = percentage(*value);
            block.fraction = *value;
        }
        WidgetView::List {
            items,
            hidden_item_count,
        } => {
            block.kind = LIST_KIND;
            block.detail = items
                .iter()
                .map(|item| format!("• {item}"))
                .collect::<Vec<_>>()
                .join("\n");
            if *hidden_item_count > 0 {
                if !block.detail.is_empty() {
                    block.detail.push('\n');
                }
                block
                    .detail
                    .push_str(&format!("… {hidden_item_count} more"));
            }
        }
        WidgetView::Chart {
            label,
            normalized_points,
        } => {
            block.kind = CHART_KIND;
            block.label.clone_from(label);
            block.points = downsample(normalized_points, DEFAULT.max_chart_samples);
        }
    }
    block
}

fn tone_value(tone: SemanticTone) -> i32 {
    match tone {
        SemanticTone::Neutral => 0,
        SemanticTone::Success => 1,
        SemanticTone::Warning => 2,
        SemanticTone::Danger => 3,
    }
}

fn percentage(value: f32) -> String {
    format!("{:.0}%", value * 100.0)
}

fn downsample(points: &[f32], max_samples: usize) -> Vec<f32> {
    if points.len() <= max_samples {
        return points.to_vec();
    }
    (0..max_samples)
        .map(|sample| points[sample * points.len() / max_samples])
        .collect()
}

#[cfg(test)]
mod tests {
    use panel_model::{
        ChartWidget, GaugeWidget, ListWidget, MAX_SECTIONS, MAX_WIDGETS, MetricWidget, PageId,
        ProgressWidget, Section, StatusTone, StatusWidget, TextWidget, Widget,
    };

    use super::*;

    fn page(sections: Vec<Section>) -> PageModel {
        PageModel {
            schema_version: 1,
            id: PageId::new("plugin.renderer").unwrap(),
            title: "Renderer".into(),
            icon: Some("dashboard".into()),
            sections,
            updated_at_unix_ms: None,
        }
    }

    fn all_widgets() -> PageModel {
        page(vec![Section {
            title: Some("Everything".into()),
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
                    text: "A long semantic description".into(),
                }),
                Widget::Progress(ProgressWidget {
                    label: "Build".into(),
                    value: 0.5,
                }),
                Widget::List(ListWidget {
                    items: (0..12).map(|index| format!("item {index}")).collect(),
                }),
                Widget::Chart(ChartWidget {
                    label: "Load".into(),
                    points: (0..128).map(|index| f64::from(index % 17)).collect(),
                }),
            ],
        }])
    }

    #[test]
    fn creates_a_real_render_block_for_every_widget_variant() {
        let payload = build_payload(&all_widgets(), RendererConfig::default()).unwrap();
        let kinds = payload.layouts[2]
            .blocks
            .iter()
            .map(|block| block.kind)
            .collect::<Vec<_>>();

        assert_eq!(kinds, vec![0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(payload.layouts[0].columns, 1);
        assert_eq!(payload.layouts[1].columns, 2);
        assert_eq!(payload.layouts[2].columns, 3);
        assert!(payload.layouts[0].content_height > payload.layouts[2].content_height);
    }

    #[test]
    fn preserves_empty_sections_as_visible_placeholders() {
        let payload = build_payload(
            &page(vec![Section {
                title: Some("Empty".into()),
                widgets: Vec::new(),
            }]),
            RendererConfig::default(),
        )
        .unwrap();

        assert_eq!(
            payload.layouts[0]
                .blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![SECTION_KIND, EMPTY_SECTION_KIND]
        );
    }

    #[test]
    fn bounds_lists_charts_and_long_text_before_slint() {
        let payload = build_payload(&all_widgets(), RendererConfig::default()).unwrap();
        let blocks = &payload.layouts[2].blocks;
        let list = blocks.iter().find(|block| block.kind == LIST_KIND).unwrap();
        let chart = blocks
            .iter()
            .find(|block| block.kind == CHART_KIND)
            .unwrap();

        assert!(list.detail.contains("… 4 more"));
        assert_eq!(chart.points.len(), DEFAULT.max_chart_samples);
        assert!(
            blocks
                .iter()
                .all(|block| block.label.chars().count() <= DEFAULT.max_text_chars)
        );
    }

    #[test]
    fn handles_maximum_valid_page_without_unbounded_render_models() {
        let mut sections = (0..MAX_SECTIONS)
            .map(|index| Section {
                title: Some(format!("section {index}")),
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

        let payload = build_payload(&page(sections), RendererConfig::default()).unwrap();

        for layout in &payload.layouts {
            assert_eq!(layout.blocks.len(), MAX_WIDGETS + MAX_SECTIONS);
            assert!(layout.content_height.is_finite());
        }
    }

    #[test]
    fn folds_many_indicators_and_keeps_real_page_indices() {
        let payload = build_payload(
            &page(Vec::new()),
            RendererConfig {
                page_count: 100,
                selected_page: 50,
                ..RendererConfig::default()
            },
        )
        .unwrap();

        assert_eq!(payload.indicators.len(), DEFAULT.max_visible_indicators + 2);
        assert_eq!(payload.indicators.first().unwrap().label, "…");
        assert_eq!(payload.indicators.last().unwrap().label, "…");
        assert!(
            payload
                .indicators
                .iter()
                .any(|item| item.page_index == 50 && item.selected)
        );
    }

    #[test]
    fn carries_scale_and_reduce_motion_to_the_slint_payload() {
        let payload = build_payload(
            &page(Vec::new()),
            RendererConfig {
                scale: 1.75,
                reduce_motion: true,
                ..RendererConfig::default()
            },
        )
        .unwrap();

        assert_eq!(payload.scale, 1.75);
        assert!(payload.reduce_motion);
    }
}
