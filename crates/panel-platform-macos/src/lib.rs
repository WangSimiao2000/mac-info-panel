//! macOS platform adapter skeleton.
//!
//! Native AppKit, display, tray, autostart, and media integrations will replace the explicit
//! unsupported implementation in focused follow-up work.

#[cfg(target_os = "macos")]
mod metrics;
#[cfg(target_os = "macos")]
pub use metrics::MacOsMetricsSource;

use panel_platform::{
    AutostartService, DisplayService, MediaService, PlatformCapabilities, PlatformKind,
    PlatformServices, PlatformSkeleton, TrayService, WindowService,
};

pub struct MacOsPlatform {
    skeleton: PlatformSkeleton,
}

impl MacOsPlatform {
    pub const fn new() -> Self {
        Self {
            skeleton: PlatformSkeleton::new(PlatformKind::MacOs),
        }
    }
}

impl Default for MacOsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformServices for MacOsPlatform {
    fn platform(&self) -> PlatformKind {
        PlatformKind::MacOs
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
        let platform = MacOsPlatform::new();
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
