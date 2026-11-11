# Vendored nih_plug

Copied from https://github.com/robbert-vdh/nih-plug at rev
`de421011f41a6d10fc8c7a6084e4f4dee0143683` (the `nih_plug` crate only, ISC
license, see LICENSE) and wired in with `[patch]` in the workspace
`Cargo.toml`. The other nih-plug crates (`nih_plug_derive`, `nih_plug_egui`,
`nih_plug_xtask`) still come from git at the same rev and pick up this copy
through the patch. Changes:

1. `Cargo.toml`: dropped upstream's `[workspace]` and `[profile.*]` sections,
   pointed `nih_plug_derive` at the git rev, and allowed two upstream lints
   (`unexpected_cfgs`, `unused_imports`) that cargo stops capping once the
   crate is a path dependency.
2. CLAP `ext_state_load` (`src/wrapper/clap/wrapper.rs`): the state's u64
   length prefix comes from the host stream and was passed straight to
   `Vec::with_capacity`, so a corrupt or foreign state aborted the host
   ("memory allocation of 1025222176999353387 bytes failed"; clap-validator
   `state-invalid-random` crashed, for nih-plug's own examples too). It now
   reads in bounded 64 KiB chunks and never allocates more than the stream
   actually delivered; a short stream returns `false` as before.
3. CLAP `set_state_inner` (same file): after a state load the parameter
   values change, but upstream only notified the editor, never the host, so
   hosts kept showing stale values (clap-validator 0.4
   `state-reproducibility-{basic,binary,buffered}` failed, for nih-plug's own
   gain example too). It now also schedules `Task::RescanParamValues`
   (`clap_host_params::rescan(CLAP_PARAM_RESCAN_VALUES)`). The GUI-context
   `set_state` path already did this, so there it now rescans twice, which
   is harmless.

4. Host-driven editor resizing (`src/editor.rs`, `src/wrapper/vst3/view.rs`,
   `src/wrapper/clap/wrapper.rs`): upstream editors are fixed-size (every
   resize entry point is a `TODO`). `Editor` gains three default methods,
   `can_resize()` (false), `constrain_size(w, h)` and `set_size(w, h)`
   (logical pixels), so existing editors behave as before. For an editor
   that opts in, VST3 `canResize` / `checkSizeConstraint` / `onSize` and
   CLAP `can_resize` / `get_resize_hints` / `adjust_size` / `set_size` ask
   it (unscaling by the host DPI factor on Windows/Linux). chromagrain's
   editor clamps to 960×600..3840×2400.

Drop this directory and the `[patch]` once the fixes are upstream.
