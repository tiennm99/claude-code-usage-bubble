//! macOS-only helpers. The macOS build is a compile target for now; these
//! stubs keep the portable callers working until the platform is finished.

/// Whether the primary mouse button is held. Always false for now, so a
/// drag ends 250 ms after the last move; `NSEvent.pressedMouseButtons`
/// would let a paused drag keep the debounce armed, as on X11.
pub fn primary_button_down() -> bool {
    false
}

/// Whether a fullscreen window of another app is frontmost. Always false
/// for now; `CGWindowListCopyWindowInfo` with the on-screen window list
/// (layer 0, bounds covering a display, owner PID not ours) would answer it.
pub fn foreground_is_fullscreen() -> super::FullscreenCheck {
    super::FullscreenCheck::new(false, "not implemented on macOS")
}
