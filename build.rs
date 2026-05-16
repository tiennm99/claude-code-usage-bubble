// Compiles `res/icon.rc` into the binary as Windows PE resources.
//
// `embed-resource` shells out to `rc.exe` (when a Windows SDK is on PATH) or
// `windres` (MinGW) to produce a .res object that the linker bakes into the
// .exe alongside our Rust code. The .rc file references `src/icons/icon.ico`
// via a relative path; keep that path stable.

fn main() {
    // `compile` returns a `CompilationResult` annotated `#[must_use]` on
    // embed-resource 3.x. `manifest_optional()` collapses Linux/Mac no-op
    // cases (where there's no `rc.exe`) into Ok while still surfacing real
    // failures on Windows.
    embed_resource::compile("res/icon.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("Failed to compile Windows resources");
}
