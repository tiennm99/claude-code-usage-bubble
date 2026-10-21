//! How this copy of the app was installed, which decides whether an update
//! can be applied in place.

use std::ffi::OsString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// The Windows portable exe: the download replaces it via `self_replace`.
    Portable,
    /// A Linux AppImage (`$APPIMAGE` set): the updater plugin swaps the file.
    AppImage,
    /// A Linux install owned by a package manager (`.deb`): the check runs,
    /// applying is left to the package manager.
    PackageManager,
    /// No in-place update path yet (macOS).
    Unsupported,
}

impl Channel {
    /// Whether "Update available" may download and swap the binary.
    pub fn can_apply(self) -> bool {
        matches!(self, Channel::Portable | Channel::AppImage)
    }
}

/// The channel of the running process.
pub fn current() -> Channel {
    detect(std::env::consts::OS, std::env::var_os("APPIMAGE"))
}

/// The channel for an OS name (`std::env::consts::OS`) and the `APPIMAGE`
/// variable the AppImage runtime sets. An empty `APPIMAGE` counts as unset.
pub fn detect(os: &str, appimage: Option<OsString>) -> Channel {
    match os {
        "windows" => Channel::Portable,
        "linux" if appimage.is_some_and(|v| !v.is_empty()) => Channel::AppImage,
        "linux" => Channel::PackageManager,
        _ => Channel::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &str) -> Option<OsString> {
        Some(OsString::from(v))
    }

    #[test]
    fn channel_follows_the_os_and_appimage_variable() {
        assert_eq!(detect("windows", None), Channel::Portable);
        // A stray APPIMAGE on Windows does not change the portable path.
        assert_eq!(detect("windows", os("/x.AppImage")), Channel::Portable);
        assert_eq!(
            detect("linux", os("/home/u/App.AppImage")),
            Channel::AppImage
        );
        assert_eq!(detect("linux", None), Channel::PackageManager);
        assert_eq!(detect("linux", os("")), Channel::PackageManager);
        assert_eq!(detect("macos", None), Channel::Unsupported);
        assert_eq!(detect("macos", os("/x.AppImage")), Channel::Unsupported);
    }

    #[test]
    fn only_portable_and_appimage_apply_in_place() {
        assert!(Channel::Portable.can_apply());
        assert!(Channel::AppImage.can_apply());
        assert!(!Channel::PackageManager.can_apply());
        assert!(!Channel::Unsupported.can_apply());
    }
}
