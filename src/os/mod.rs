// `os` namespace: thin, typed wrappers over the slice of Win32 we use.
//
// Each submodule covers one concern (color conversion, UTF-16 strings,
// DPI math, registry I/O, theme detection). Nothing in here knows about
// the bubble UI or the polling loop — it's pure platform glue.

pub mod color;
pub mod dpi;
pub mod registry;
pub mod string;
pub mod theme;

pub use color::Rgb;
pub use string::to_utf16_nul;
