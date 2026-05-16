// `net` namespace: HTTP client implementations.
//
// `winhttp` is the only backend right now (Windows-only app). It produces
// `Response` values that are cheap to inspect via `status`, `header`,
// `text`, and `json`.

pub mod winhttp;

pub use winhttp::{Client, Error, Response};
