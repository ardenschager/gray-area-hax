# clap-validator 0.4.1 on target/bundled/chromagrain.clap (v002)

`clap-validator validate target/bundled/chromagrain.clap` (headless), with the
vendored nih-plug fixes (vendor/nih-plug/PATCHES.md).

**44 tests run, 36 passed, 0 failed, 0 warnings, 8 skipped**

Changes vs v001 (32 passed, 4 failed): `state-invalid-random` no longer crashes
(bounded state read), and the three `state-reproducibility-*` tests pass
(the host is asked to rescan parameter values after a state load). Skip
reasons are my reading of the validator's descriptions.

| test | result |
|---|---|
| query-nonexistent-factory | PASSED |
| create-id-with-trailing-garbage | PASSED |
| scan-rtld-now | PASSED |
| scan-time | PASSED |
| preset-discovery-crawl | SKIPPED (no preset discovery factory) |
| preset-discovery-descriptor-consistency | SKIPPED (no preset discovery factory) |
| preset-discovery-load | SKIPPED (no preset discovery factory) |
| descriptor-consistency | PASSED |
| features-categories | PASSED |
| features-duplicates | PASSED |
| process-audio-basic-out-of-place | PASSED |
| process-audio-basic-in-place | PASSED |
| process-audio-double-out-of-place | SKIPPED (no 64-bit audio) |
| process-audio-double-in-place | SKIPPED (no 64-bit audio) |
| process-audio-denormals | SKIPPED (instrument has no audio input) |
| process-sleep-constant-mask | PASSED |
| process-sleep-process-status | PASSED |
| process-note-out-of-place-basic | PASSED |
| process-note-inconsistent | PASSED |
| process-note-wildcard | PASSED |
| process-varying-sample-rates | PASSED |
| process-varying-block-sizes | PASSED |
| process-random-block-sizes | PASSED |
| process-reset-reactivate | PASSED |
| layout-audio-ports-activation | SKIPPED (single fixed layout) |
| layout-audio-ports-config | PASSED |
| layout-configurable-audio-ports | SKIPPED (single fixed layout) |
| param-set-events | PASSED |
| param-set-no-cookies | PASSED |
| param-set-wrong-namespace | PASSED |
| param-fuzz-basic | PASSED |
| param-fuzz-bounds | PASSED |
| param-fuzz-sample-accurate | PASSED |
| param-fuzz-modulation | PASSED |
| param-conversions | PASSED |
| param-default-values | PASSED |
| state-invalid-empty | PASSED |
| state-invalid-random | PASSED |
| state-reproducibility-basic | PASSED |
| state-reproducibility-binary | PASSED |
| state-reproducibility-buffered | PASSED |
| transport-null | PASSED |
| transport-fuzz | PASSED |
| transport-fuzz-sample-accurate | PASSED |
