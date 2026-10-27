//! Linux platform adapter skeleton.
//!
//! Native Wayland/X11, StatusNotifierItem, XDG autostart, and MPRIS integrations will replace the
//! explicit unsupported implementation in focused follow-up work.

#[cfg(target_os = "linux")]
mod metrics;
#[cfg(target_os = "linux")]
pub use metrics::LinuxMetricsSource;

use panel_platform::{
    AutostartService, DisplayService, MediaService, PlatformCapabilities, PlatformKind,
    PlatformServices, PlatformSkeleton, TrayService, WindowService,
};

pub struct LinuxPlatform {
    skeleton: PlatformSkeleton,
}

impl LinuxPlatform {
    pub const fn new() -> Self {
        Self {
            skeleton: PlatformSkeleton::new(PlatformKind::Linux),
        }
    }
}

impl Default for LinuxPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformServices for LinuxPlatform {
    fn platform(&self) -> PlatformKind {
        PlatformKind::Linux
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
        let platform = LinuxPlatform::new();
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
