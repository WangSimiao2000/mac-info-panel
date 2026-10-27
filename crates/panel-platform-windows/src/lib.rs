//! Windows platform adapter skeleton.
//!
//! Native Win32, display, tray, autostart, and GSMTC integrations will replace the explicit
//! unsupported implementation in focused follow-up work.

#[cfg(target_os = "windows")]
mod metrics;
#[cfg(target_os = "windows")]
pub use metrics::WindowsMetricsSource;

use panel_platform::{
    AutostartService, DisplayService, MediaService, PlatformCapabilities, PlatformKind,
    PlatformServices, PlatformSkeleton, TrayService, WindowService,
};

pub struct WindowsPlatform {
    skeleton: PlatformSkeleton,
}

impl WindowsPlatform {
    pub const fn new() -> Self {
        Self {
            skeleton: PlatformSkeleton::new(PlatformKind::Windows),
        }
    }
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformServices for WindowsPlatform {
    fn platform(&self) -> PlatformKind {
        PlatformKind::Windows
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::unavailable()
    }

    fn window(&self) -> &dyn WindowService {
        PlatformServices::window(&self.skeleton)
    }

    fn displays(&self) -> &dyn DisplayService {
        PlatformServices::displays(&self.skeleton)
    }

    fn tray(&self) -> &dyn TrayService {
        PlatformServices::tray(&self.skeleton)
    }

    fn autostart(&self) -> &dyn AutostartService {
        PlatformServices::autostart(&self.skeleton)
    }

    fn media(&self) -> &dyn MediaService {
        PlatformServices::media(&self.skeleton)
    }
}

#[cfg(test)]
mod tests {
    use panel_platform::{PlatformErrorKind, SupportStatus};

    use super::*;

    #[test]
    fn skeleton_reports_unimplemented_services_truthfully() {
        let platform = WindowsPlatform::new();
        assert_eq!(
            platform.capabilities().window.service,
            SupportStatus::Unsupported
        );
        assert_eq!(
            platform.media().current_playback().unwrap_err().kind,
            PlatformErrorKind::Unsupported
        );
    }
}
