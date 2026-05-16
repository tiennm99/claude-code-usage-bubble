// UTF-16 conversion helpers.

/// Encode a Rust `&str` as a NUL-terminated UTF-16 vector suitable for
/// passing to Win32 `PCWSTR`-typed parameters.
///
/// The result lives as long as the returned `Vec<u16>`; callers must keep
/// the vector alive across the FFI call.
pub fn to_utf16_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
