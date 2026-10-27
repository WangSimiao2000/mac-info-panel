use std::collections::BTreeMap;

use panel_config::{AppConfig, ConfigError, PageKind};
use panel_model::{BuiltInPage, PageDescriptor, PageId, PageModel, PageSource, PluginId};
use thiserror::Error;

const MAX_DIAGNOSTICS: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub struct AppState {
    pub pages: Vec<PageDescriptor>,
    pub selected_page: PageId,
    pub visible: bool,
    pub plugins: BTreeMap<PluginId, PluginState>,
    pub diagnostics: Vec<Diagnostic>,
    auto_rotate_ms: Option<u64>,
    next_rotation_at_ms: Option<u64>,
}

impl AppState {
    pub fn from_config(config: &AppConfig, now_ms: u64) -> Result<Self, ConfigStateError> {
        config.validate()?;
        let pages = config
            .pages
            .iter()
            .enumerate()
            .map(|(index, page)| {
                let id = PageId::new(page.id.clone()).map_err(|error| {
                    ConfigStateError::InvalidField {
                        field: format!("pages[{index}].id"),
                        reason: error.to_string(),
                    }
                })?;
                let source = match page.kind {
                    PageKind::BuiltIn => PageSource::BuiltIn {
                        page: BuiltInPage::from_id(id.as_str()).ok_or_else(|| {
                            ConfigStateError::InvalidField {
                                field: format!("pages[{index}].id"),
                                reason: "is not a recognized built-in page identifier".into(),
                            }
                        })?,
                    },
                    PageKind::Plugin => PageSource::Plugin {
                        plugin_id: PluginId::new(page.id.clone()).map_err(|error| {
                            ConfigStateError::InvalidField {
                                field: format!("pages[{index}].id"),
                                reason: error.to_string(),
                            }
                        })?,
                    },
                };
                Ok(PageDescriptor {
                    id,
                    title: page.title.clone(),
                    source,
                })
            })
            .collect::<Result<Vec<_>, ConfigStateError>>()?;
        let initial_page = PageId::new(config.behavior.initial_page.clone()).map_err(|error| {
            ConfigStateError::InvalidField {
                field: "behavior.initialPage".into(),
                reason: error.to_string(),
            }
        })?;
        Self::new(
            pages,
            initial_page,
            config.behavior.auto_rotate_seconds,
            now_ms,
        )
        .map_err(ConfigStateError::Core)
    }

    pub fn new(
        pages: Vec<PageDescriptor>,
        initial_page: PageId,
        auto_rotate_seconds: u64,
        now_ms: u64,
    ) -> Result<Self, CoreError> {
        if pages.is_empty() {
            return Err(CoreError::NoPages);
        }
        if !pages.iter().any(|page| page.id == initial_page) {
            return Err(CoreError::UnknownPage(initial_page.as_str().into()));
        }
        let interval = auto_rotate_seconds
            .checked_mul(1_000)
            .filter(|value| *value > 0);
        Ok(Self {
            pages,
            selected_page: initial_page,
            visible: true,
            plugins: BTreeMap::new(),
            diagnostics: Vec::new(),
            auto_rotate_ms: interval,
            next_rotation_at_ms: interval.and_then(|value| now_ms.checked_add(value)),
        })
    }
    pub fn reduce(&mut self, event: AppEvent) -> Result<(), CoreError> {
        match event {
            AppEvent::SelectPage { page, now_ms } => {
                let index = self.page_index(&page)?;
                self.selected_page = self.pages[index].id.clone();
                self.reset_rotation(now_ms);
            }
            AppEvent::NextPage { now_ms } => {
                self.move_by(1);
                self.reset_rotation(now_ms);
            }
            AppEvent::PreviousPage { now_ms } => {
                self.move_by(-1);
                self.reset_rotation(now_ms);
            }
            AppEvent::Tick { now_ms } => {
                if self.visible
                    && self
                        .next_rotation_at_ms
                        .is_some_and(|deadline| now_ms >= deadline)
                {
                    self.move_by(1);
                    self.reset_rotation(now_ms);
                }
            }
            AppEvent::VisibilityChanged { visible, now_ms } => {
                self.visible = visible;
                self.next_rotation_at_ms = if visible {
                    self.deadline_after(now_ms)
                } else {
                    None
                };
            }
            AppEvent::PluginRefreshStarted {
                plugin_id,
                generation,
            } => {
                let state = self.plugins.entry(plugin_id).or_default();
                if generation >= state.generation {
                    state.generation = generation;
                    state.status = PluginStatus::Loading;
                }
            }
            AppEvent::PluginRefreshed {
                plugin_id,
                generation,
                page,
            } => {
                if self.is_current_generation(&plugin_id, generation) {
                    match page.validate() {
                        Ok(()) => {
                            self.plugins.entry(plugin_id).or_default().status =
                                PluginStatus::Ready(page);
                        }
                        Err(error) => {
                            self.plugins.entry(plugin_id.clone()).or_default().status =
                                PluginStatus::Failed(error.to_string());
                            self.push_diagnostic(Diagnostic::error(
                                "plugin.invalid-page",
                                Some(plugin_id),
                                error.to_string(),
                            ));
                        }
                    }
                }
            }
            AppEvent::PluginFailed {
                plugin_id,
                generation,
                message,
            } => {
                if self.is_current_generation(&plugin_id, generation) {
                    self.plugins.entry(plugin_id.clone()).or_default().status =
                        PluginStatus::Failed(message.clone());
                    self.push_diagnostic(Diagnostic::error(
                        "plugin.refresh-failed",
                        Some(plugin_id),
                        message,
                    ));
                }
            }
            AppEvent::ClearDiagnostics => self.diagnostics.clear(),
        }
        Ok(())
    }

    fn page_index(&self, page: &PageId) -> Result<usize, CoreError> {
        self.pages
            .iter()
            .position(|candidate| &candidate.id == page)
            .ok_or_else(|| CoreError::UnknownPage(page.as_str().into()))
    }
    fn move_by(&mut self, offset: isize) {
        let current = self.page_index(&self.selected_page).unwrap_or(0) as isize;
        let next = (current + offset).rem_euclid(self.pages.len() as isize) as usize;
        self.selected_page = self.pages[next].id.clone();
    }

    fn reset_rotation(&mut self, now_ms: u64) {
        self.next_rotation_at_ms = if self.visible {
            self.deadline_after(now_ms)
        } else {
            None
        };
    }

    fn deadline_after(&self, now_ms: u64) -> Option<u64> {
        self.auto_rotate_ms
            .and_then(|interval| now_ms.checked_add(interval))
    }

    fn is_current_generation(&self, plugin_id: &PluginId, generation: u64) -> bool {
        self.plugins
            .get(plugin_id)
            .is_some_and(|state| state.generation == generation)
    }

    fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() == MAX_DIAGNOSTICS {
            self.diagnostics.remove(0);
        }
        self.diagnostics.push(diagnostic);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppEvent {
    SelectPage {
        page: PageId,
        now_ms: u64,
    },
    NextPage {
        now_ms: u64,
    },
    PreviousPage {
        now_ms: u64,
    },
    Tick {
        now_ms: u64,
    },
    VisibilityChanged {
        visible: bool,
        now_ms: u64,
    },
    PluginRefreshStarted {
        plugin_id: PluginId,
        generation: u64,
    },
    PluginRefreshed {
        plugin_id: PluginId,
        generation: u64,
        page: PageModel,
    },
    PluginFailed {
        plugin_id: PluginId,
        generation: u64,
        message: String,
    },
    ClearDiagnostics,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PluginState {
    pub generation: u64,
    pub status: PluginStatus,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum PluginStatus {
    #[default]
    Idle,
    Loading,
    Ready(PageModel),
    Failed(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigStateError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("invalid field {field}: {reason}")]
    InvalidField { field: String, reason: String },
    #[error(transparent)]
    Core(CoreError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub plugin_id: Option<PluginId>,
    pub message: String,
}

impl Diagnostic {
    fn error(code: &'static str, plugin_id: Option<PluginId>, message: String) -> Self {
        Self {
            code,
            plugin_id,
            message,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("at least one page is required")]
    NoPages,
    #[error("page does not exist: {0}")]
    UnknownPage(String),
}

pub fn checked_counter_delta(current: u64, previous: u64) -> Option<u64> {
    current.checked_sub(previous)
}

#[cfg(test)]
mod tests {
    use panel_model::{BuiltInPage, GaugeWidget, PageSource, Section, Widget};

    use super::*;

    fn id(value: &str) -> PageId {
        PageId::new(value).unwrap()
    }

    fn plugin_id() -> PluginId {
        PluginId::new("example.status").unwrap()
    }

    fn page(id_value: &str) -> PageDescriptor {
        PageDescriptor {
            id: id(id_value),
            title: id_value.into(),
            source: PageSource::BuiltIn {
                page: BuiltInPage::Overview,
            },
        }
    }

    fn state(interval_seconds: u64) -> AppState {
        AppState::new(
            vec![page("overview"), page("local")],
            id("overview"),
            interval_seconds,
            0,
        )
        .unwrap()
    }

    fn plugin_page(value: f64) -> PageModel {
        PageModel {
            schema_version: 1,
            id: id("plugin-page"),
            title: "Plugin".into(),
            icon: None,
            sections: vec![Section {
                title: None,
                widgets: vec![Widget::Gauge(GaugeWidget {
                    label: "Load".into(),
                    value,
                })],
            }],
            updated_at_unix_ms: None,
        }
    }
    #[test]
    fn rejects_unknown_initial_page() {
        assert_eq!(
            AppState::new(vec![page("overview")], id("missing"), 0, 0),
            Err(CoreError::UnknownPage("missing".into()))
        );
    }

    #[test]
    fn rotates_only_after_deadline_while_visible() {
        let mut value = state(10);
        value.reduce(AppEvent::Tick { now_ms: 9_999 }).unwrap();
        assert_eq!(value.selected_page.as_str(), "overview");
        value.reduce(AppEvent::Tick { now_ms: 10_000 }).unwrap();
        assert_eq!(value.selected_page.as_str(), "local");
        value
            .reduce(AppEvent::VisibilityChanged {
                visible: false,
                now_ms: 10_001,
            })
            .unwrap();
        value.reduce(AppEvent::Tick { now_ms: 30_000 }).unwrap();
        assert_eq!(value.selected_page.as_str(), "local");
    }

    #[test]
    fn ignores_stale_plugin_results() {
        let mut value = state(0);
        value
            .reduce(AppEvent::PluginRefreshStarted {
                plugin_id: plugin_id(),
                generation: 2,
            })
            .unwrap();
        value
            .reduce(AppEvent::PluginRefreshed {
                plugin_id: plugin_id(),
                generation: 1,
                page: plugin_page(0.5),
            })
            .unwrap();
        assert_eq!(value.plugins[&plugin_id()].status, PluginStatus::Loading);
    }
    #[test]
    fn rejects_invalid_plugin_output_and_records_diagnostic() {
        let mut value = state(0);
        value
            .reduce(AppEvent::PluginRefreshStarted {
                plugin_id: plugin_id(),
                generation: 1,
            })
            .unwrap();
        value
            .reduce(AppEvent::PluginRefreshed {
                plugin_id: plugin_id(),
                generation: 1,
                page: plugin_page(2.0),
            })
            .unwrap();
        assert!(matches!(
            value.plugins[&plugin_id()].status,
            PluginStatus::Failed(_)
        ));
        assert_eq!(value.diagnostics[0].code, "plugin.invalid-page");
    }

    #[test]
    fn initializes_state_from_validated_configuration() {
        let config = AppConfig::from_jsonc(
            r#"{
                "schemaVersion": 1,
                "behavior": {"initialPage":"local-system","autoRotateSeconds":15},
                "pages": [
                    {"id":"overview","title":"Overview","kind":"builtIn"},
                    {"id":"local-system","title":"Local","kind":"builtIn"}
                ]
            }"#,
        )
        .unwrap();
        let state = AppState::from_config(&config, 1_000).unwrap();
        assert_eq!(state.selected_page.as_str(), "local-system");
        assert_eq!(
            state.pages[1].source,
            PageSource::BuiltIn {
                page: BuiltInPage::LocalSystem
            }
        );
    }

    #[test]
    fn treats_decreasing_counter_as_reset() {
        assert_eq!(checked_counter_delta(5, 10), None);
        assert_eq!(checked_counter_delta(15, 10), Some(5));
    }
}
