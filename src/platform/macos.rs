//! macOS-only helpers. The macOS build is a compile target for now; these
//! stubs keep the portable callers working until the platform is finished.

/// Whether the primary mouse button is held. Always false for now, so a
/// drag ends 250 ms after the last move; `NSEvent.pressedMouseButtons`
/// would let a paused drag keep the debounce armed, as on X11.
pub fn primary_button_down() -> bool {
    false
}
