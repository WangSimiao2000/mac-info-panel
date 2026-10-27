//! Platform-neutral service contracts for Info Panel.
//!
//! Operating-system framework types belong in the platform implementation crates. Callers must
//! inspect capability reports and handle [`PlatformErrorKind::Unsupported`] rather than assuming
//! that every platform provides every feature.

use std::fmt;

/// Operating-system family implemented by a platform crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformKind {
    MacOs,
    Windows,
    Linux,
}

/// Runtime support for a platform feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportStatus {
    /// The current implementation cannot provide the feature.
    Unsupported,
    /// The feature is ready to use.
    Available,
    /// The feature exists but needs user or system permission before use.
    PermissionRequired,
}

impl SupportStatus {
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Window background effects with platform-neutral meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundMaterial {
    Blur,
    Mica,
    Acrylic,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCapabilities {
    pub service: SupportStatus,
    pub non_activating_panel: SupportStatus,
    pub all_workspaces: SupportStatus,
    pub materials: Vec<BackgroundMaterial>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayCapabilities {
    pub service: SupportStatus,
    pub stable_ids: SupportStatus,
    pub change_notifications: SupportStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayCapabilities {
    pub service: SupportStatus,
    pub menu_updates: SupportStatus,
    pub page_navigation: SupportStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutostartCapabilities {
    pub service: SupportStatus,
    pub status_query: SupportStatus,
    pub configuration: SupportStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaCapabilities {
    pub service: SupportStatus,
    pub change_notifications: SupportStatus,
    pub stable_content_id: SupportStatus,
    pub stable_player_id: SupportStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub window: WindowCapabilities,
    pub displays: DisplayCapabilities,
    pub tray: TrayCapabilities,
    pub autostart: AutostartCapabilities,
    pub media: MediaCapabilities,
    pub thermal_state: SupportStatus,
}

impl PlatformCapabilities {
    /// A truthful capability report for a crate that contains no OS implementation yet.
    pub fn unavailable() -> Self {
        Self {
            window: WindowCapabilities {
                service: SupportStatus::Unsupported,
                non_activating_panel: SupportStatus::Unsupported,
                all_workspaces: SupportStatus::Unsupported,
                materials: Vec::new(),
            },
            displays: DisplayCapabilities {
                service: SupportStatus::Unsupported,
                stable_ids: SupportStatus::Unsupported,
                change_notifications: SupportStatus::Unsupported,
            },
            tray: TrayCapabilities {
                service: SupportStatus::Unsupported,
                menu_updates: SupportStatus::Unsupported,
                page_navigation: SupportStatus::Unsupported,
            },
            autostart: AutostartCapabilities {
                service: SupportStatus::Unsupported,
                status_query: SupportStatus::Unsupported,
                configuration: SupportStatus::Unsupported,
            },
            media: MediaCapabilities {
                service: SupportStatus::Unsupported,
                change_notifications: SupportStatus::Unsupported,
                stable_content_id: SupportStatus::Unsupported,
                stable_player_id: SupportStatus::Unsupported,
            },
            thermal_state: SupportStatus::Unsupported,
        }
    }
}

/// Stable application-level identifier for a native window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRect {
    pub origin: LogicalPoint,
    pub size: LogicalSize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelWindowOptions {
    pub bounds: LogicalRect,
    pub non_activating: bool,
    pub visible_on_all_workspaces: bool,
    pub material: Option<BackgroundMaterial>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayId(String);

impl DisplayId {
    pub fn new(value: impl Into<String>) -> Result<Self, PlatformError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(PlatformError::invalid_request(
                "display-id",
                "display ID cannot be empty",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayInfo {
    pub id: DisplayId,
    pub name: String,
    pub bounds: LogicalRect,
    pub work_area: LogicalRect,
    pub scale_factor: f64,
    pub primary: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplaySelection {
    StableId(DisplayId),
    Name(String),
    PreferSecondary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    MenuItemSelected(String),
    NextPage,
    PreviousPage,
    ToggleVisibility,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutostartStatus {
    Disabled,
    Enabled,
    PermissionRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Paused,
    Playing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackMetadata {
    pub content_id: Option<String>,
    pub player_id: Option<String>,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaybackSnapshot {
    pub status: PlaybackStatus,
    pub track: Option<TrackMetadata>,
    pub position_ms: Option<u64>,
}

/// Cancellation handle for a platform event subscription.
pub trait Subscription: Send + Sync {}

pub trait DisplayEventSink: Send + Sync {
    fn displays_changed(&self);
}

pub trait TrayEventSink: Send + Sync {
    fn tray_event(&self, event: TrayEvent);
}

pub trait MediaEventSink: Send + Sync {
    fn playback_changed(&self, snapshot: PlaybackSnapshot);
}

pub trait WindowService: Send + Sync {
    fn capabilities(&self) -> WindowCapabilities;
    fn configure_panel(
        &self,
        window: WindowId,
        options: &PanelWindowOptions,
    ) -> Result<(), PlatformError>;
    fn set_visible(&self, window: WindowId, visible: bool) -> Result<(), PlatformError>;
}

pub trait DisplayService: Send + Sync {
    fn capabilities(&self) -> DisplayCapabilities;
    fn displays(&self) -> Result<Vec<DisplayInfo>, PlatformError>;
    fn subscribe(
        &self,
        sink: Box<dyn DisplayEventSink>,
    ) -> Result<Box<dyn Subscription>, PlatformError>;
}

pub trait TrayService: Send + Sync {
    fn capabilities(&self) -> TrayCapabilities;
    fn set_menu(&self, items: &[TrayMenuItem]) -> Result<(), PlatformError>;
    fn subscribe(
        &self,
        sink: Box<dyn TrayEventSink>,
    ) -> Result<Box<dyn Subscription>, PlatformError>;
}

pub trait AutostartService: Send + Sync {
    fn capabilities(&self) -> AutostartCapabilities;
    fn status(&self) -> Result<AutostartStatus, PlatformError>;
    fn set_enabled(&self, enabled: bool) -> Result<(), PlatformError>;
}

pub trait MediaService: Send + Sync {
    fn capabilities(&self) -> MediaCapabilities;
    fn current_playback(&self) -> Result<PlaybackSnapshot, PlatformError>;
    fn subscribe(
        &self,
        sink: Box<dyn MediaEventSink>,
    ) -> Result<Box<dyn Subscription>, PlatformError>;
}

/// Complete service set selected by the desktop application at startup.
pub trait PlatformServices: Send + Sync {
    fn platform(&self) -> PlatformKind;
    fn capabilities(&self) -> PlatformCapabilities;
    fn window(&self) -> &dyn WindowService;
    fn displays(&self) -> &dyn DisplayService;
    fn tray(&self) -> &dyn TrayService;
    fn autostart(&self) -> &dyn AutostartService;
    fn media(&self) -> &dyn MediaService;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformErrorKind {
    Unsupported,
    PermissionDenied,
    Unavailable,
    InvalidRequest,
    Backend,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformError {
    pub kind: PlatformErrorKind,
    pub operation: &'static str,
    pub message: String,
}

impl PlatformError {
    pub fn unsupported(operation: &'static str) -> Self {
        Self {
            kind: PlatformErrorKind::Unsupported,
            operation,
            message: "platform capability is not implemented".into(),
        }
    }

    pub fn invalid_request(operation: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: PlatformErrorKind::InvalidRequest,
            operation,
            message: message.into(),
        }
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for PlatformError {}

/// Reusable implementation for a platform crate before native behavior lands.
///
/// Every capability is reported as unsupported and every operation returns an explicit error.
pub struct PlatformSkeleton {
    platform: PlatformKind,
}

impl PlatformSkeleton {
    pub const fn new(platform: PlatformKind) -> Self {
        Self { platform }
    }
}

impl WindowService for PlatformSkeleton {
    fn capabilities(&self) -> WindowCapabilities {
        PlatformCapabilities::unavailable().window
    }

    fn configure_panel(
        &self,
        _window: WindowId,
        _options: &PanelWindowOptions,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported("window.configure-panel"))
    }

    fn set_visible(&self, _window: WindowId, _visible: bool) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported("window.set-visible"))
    }
}

impl DisplayService for PlatformSkeleton {
    fn capabilities(&self) -> DisplayCapabilities {
        PlatformCapabilities::unavailable().displays
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>, PlatformError> {
        Err(PlatformError::unsupported("display.list"))
    }

    fn subscribe(
        &self,
        _sink: Box<dyn DisplayEventSink>,
    ) -> Result<Box<dyn Subscription>, PlatformError> {
        Err(PlatformError::unsupported("display.subscribe"))
    }
}

impl TrayService for PlatformSkeleton {
    fn capabilities(&self) -> TrayCapabilities {
        PlatformCapabilities::unavailable().tray
    }

    fn set_menu(&self, _items: &[TrayMenuItem]) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported("tray.set-menu"))
    }

    fn subscribe(
        &self,
        _sink: Box<dyn TrayEventSink>,
    ) -> Result<Box<dyn Subscription>, PlatformError> {
        Err(PlatformError::unsupported("tray.subscribe"))
    }
}

impl AutostartService for PlatformSkeleton {
    fn capabilities(&self) -> AutostartCapabilities {
        PlatformCapabilities::unavailable().autostart
    }

    fn status(&self) -> Result<AutostartStatus, PlatformError> {
        Err(PlatformError::unsupported("autostart.status"))
    }

    fn set_enabled(&self, _enabled: bool) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported("autostart.set-enabled"))
    }
}

impl MediaService for PlatformSkeleton {
    fn capabilities(&self) -> MediaCapabilities {
        PlatformCapabilities::unavailable().media
    }

    fn current_playback(&self) -> Result<PlaybackSnapshot, PlatformError> {
        Err(PlatformError::unsupported("media.current-playback"))
    }

    fn subscribe(
        &self,
        _sink: Box<dyn MediaEventSink>,
    ) -> Result<Box<dyn Subscription>, PlatformError> {
        Err(PlatformError::unsupported("media.subscribe"))
    }
}

impl PlatformServices for PlatformSkeleton {
    fn platform(&self) -> PlatformKind {
        self.platform
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::unavailable()
    }

    fn window(&self) -> &dyn WindowService {
        self
    }

    fn displays(&self) -> &dyn DisplayService {
        self
    }

    fn tray(&self) -> &dyn TrayService {
        self
    }

    fn autostart(&self) -> &dyn AutostartService {
        self
    }

    fn media(&self) -> &dyn MediaService {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_never_claims_unimplemented_capabilities() {
        let platform = PlatformSkeleton::new(PlatformKind::Linux);

        assert_eq!(
            PlatformServices::capabilities(&platform),
            PlatformCapabilities::unavailable()
        );
        assert_eq!(
            PlatformServices::displays(&platform)
                .displays()
                .unwrap_err()
                .kind,
            PlatformErrorKind::Unsupported
        );
        assert!(
            !PlatformServices::window(&platform)
                .capabilities()
                .service
                .is_available()
        );
    }

    #[test]
    fn display_ids_reject_empty_values() {
        assert_eq!(
            DisplayId::new("  ").unwrap_err().kind,
            PlatformErrorKind::InvalidRequest
        );
    }
}
