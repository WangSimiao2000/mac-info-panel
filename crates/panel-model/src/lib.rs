use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_SECTIONS: usize = 32;
pub const MAX_WIDGETS: usize = 128;
pub const MAX_TEXT_CHARS: usize = 4_096;
pub const MAX_CHART_POINTS: usize = 1_024;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageId(String);

impl PageId {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PluginId(String);

impl PluginId {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDescriptor {
    pub id: PageId,
    pub title: String,
    pub source: PageSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PageSource {
    BuiltIn { page: BuiltInPage },
    Plugin { plugin_id: PluginId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BuiltInPage {
    Overview,
    LocalSystem,
    Lyrics,
}

impl BuiltInPage {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::LocalSystem => "local-system",
            Self::Lyrics => "lyrics",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "overview" => Some(Self::Overview),
            "local-system" => Some(Self::LocalSystem),
            "lyrics" => Some(Self::Lyrics),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageModel {
    pub schema_version: u32,
    pub id: PageId,
    pub title: String,
    pub icon: Option<String>,
    pub sections: Vec<Section>,
    pub updated_at_unix_ms: Option<i64>,
}

impl PageModel {
    pub fn validate(&self) -> Result<(), ModelError> {
        validate_identifier(self.id.as_str())?;
        validate_text("page.title", &self.title)?;
        if self.sections.len() > MAX_SECTIONS {
            return Err(ModelError::TooManySections(self.sections.len()));
        }

        let widget_count = self
            .sections
            .iter()
            .map(|section| section.widgets.len())
            .sum();
        if widget_count > MAX_WIDGETS {
            return Err(ModelError::TooManyWidgets(widget_count));
        }

        for section in &self.sections {
            if let Some(title) = &section.title {
                validate_text("section.title", title)?;
            }
            for widget in &section.widgets {
                widget.validate()?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub title: Option<String>,
    pub widgets: Vec<Widget>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Widget {
    Metric(MetricWidget),
    Status(StatusWidget),
    Gauge(GaugeWidget),
    Text(TextWidget),
    Progress(ProgressWidget),
    List(ListWidget),
    Chart(ChartWidget),
}

impl Widget {
    fn validate(&self) -> Result<(), ModelError> {
        match self {
            Self::Metric(value) => {
                validate_text("metric.label", &value.label)?;
                validate_text("metric.value", &value.value)
            }
            Self::Status(value) => {
                validate_text("status.label", &value.label)?;
                validate_text("status.value", &value.value)
            }
            Self::Gauge(value) => {
                validate_text("gauge.label", &value.label)?;
                validate_finite("gauge.value", value.value)?;
                if !(0.0..=1.0).contains(&value.value) {
                    return Err(ModelError::OutOfRange("gauge.value"));
                }
                Ok(())
            }
            Self::Text(value) => validate_text("text.text", &value.text),
            Self::Progress(value) => {
                validate_text("progress.label", &value.label)?;
                validate_finite("progress.value", value.value)?;
                if !(0.0..=1.0).contains(&value.value) {
                    return Err(ModelError::OutOfRange("progress.value"));
                }
                Ok(())
            }
            Self::List(value) => {
                for item in &value.items {
                    validate_text("list.item", item)?;
                }
                Ok(())
            }
            Self::Chart(value) => {
                validate_text("chart.label", &value.label)?;
                if value.points.len() > MAX_CHART_POINTS {
                    return Err(ModelError::TooManyChartPoints(value.points.len()));
                }
                for point in &value.points {
                    validate_finite("chart.point", *point)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricWidget {
    pub label: String,
    pub value: String,
    pub unit: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatusWidget {
    pub label: String,
    pub value: String,
    pub tone: StatusTone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusTone {
    Neutral,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GaugeWidget {
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextWidget {
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProgressWidget {
    pub label: String,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListWidget {
    pub items: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartWidget {
    pub label: String,
    pub points: Vec<f64>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("identifier must contain only ASCII letters, digits, '.', '_' or '-'")]
    InvalidIdentifier,
    #[error("{0} cannot be empty")]
    EmptyText(&'static str),
    #[error("{field} exceeds the {MAX_TEXT_CHARS} character limit")]
    TextTooLong { field: &'static str },
    #[error("page has too many sections: {0}")]
    TooManySections(usize),
    #[error("page has too many widgets: {0}")]
    TooManyWidgets(usize),
    #[error("chart has too many points: {0}")]
    TooManyChartPoints(usize),
    #[error("{0} must be finite")]
    NonFinite(&'static str),
    #[error("{0} is outside its allowed range")]
    OutOfRange(&'static str),
}

fn validate_identifier(value: &str) -> Result<(), ModelError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ModelError::InvalidIdentifier);
    }
    Ok(())
}

fn validate_text(field: &'static str, value: &str) -> Result<(), ModelError> {
    if value.trim().is_empty() {
        return Err(ModelError::EmptyText(field));
    }
    if value.chars().count() > MAX_TEXT_CHARS {
        return Err(ModelError::TextTooLong { field });
    }
    Ok(())
}

fn validate_finite(field: &'static str, value: f64) -> Result<(), ModelError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ModelError::NonFinite(field))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(widget: Widget) -> PageModel {
        PageModel {
            schema_version: 1,
            id: PageId::new("example.status").unwrap(),
            title: "Example".into(),
            icon: None,
            sections: vec![Section {
                title: None,
                widgets: vec![widget],
            }],
            updated_at_unix_ms: None,
        }
    }

    #[test]
    fn rejects_unsafe_identifiers() {
        assert_eq!(PageId::new("../plugin"), Err(ModelError::InvalidIdentifier));
    }

    #[test]
    fn rejects_unsafe_identifier_deserialized_from_plugin_output() {
        let json = r#"{
            "schemaVersion":1,
            "id":"../plugin",
            "title":"Unsafe",
            "icon":null,
            "sections":[],
            "updatedAtUnixMs":null
        }"#;
        let model: PageModel = serde_json::from_str(json).unwrap();
        assert_eq!(model.validate(), Err(ModelError::InvalidIdentifier));
    }

    #[test]
    fn rejects_non_finite_chart_points() {
        let model = page(Widget::Chart(ChartWidget {
            label: "Load".into(),
            points: vec![1.0, f64::NAN],
        }));
        assert_eq!(model.validate(), Err(ModelError::NonFinite("chart.point")));
    }

    #[test]
    fn rejects_out_of_range_progress() {
        let model = page(Widget::Progress(ProgressWidget {
            label: "CPU".into(),
            value: 1.1,
        }));
        assert_eq!(
            model.validate(),
            Err(ModelError::OutOfRange("progress.value"))
        );
    }
}
