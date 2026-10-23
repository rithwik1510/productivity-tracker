//! `bun run bindings`: writes `src/api/bindings.ts` from the same builder the app uses.
//! Never opens a window or starts the tracker.

use specta_typescript::Typescript;

fn main() {
    // Anchored to the crate dir so the output doesn't depend on the caller's cwd.
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/api/bindings.ts");
    productivity_tracker_lib::specta_builder()
        .export(Typescript::default(), out)
        .expect("failed to export TypeScript bindings");
}
