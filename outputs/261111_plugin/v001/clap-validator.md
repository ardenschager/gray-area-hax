# clap-validator 0.4.1 on target/bundled/chromagrain.clap

`clap-validator validate target/bundled/chromagrain.clap` (headless).

**44 tests run, 32 passed, 4 failed, 0 warnings, 8 skipped**

| test | result |
|---|---|
| query-nonexistent-factory | PASSED |
| create-id-with-trailing-garbage | PASSED |
| scan-rtld-now | PASSED |
| scan-time | PASSED |
| preset-discovery-crawl | SKIPPED |
| preset-discovery-descriptor-consistency | SKIPPED |
| preset-discovery-load | SKIPPED |
| descriptor-consistency | PASSED |
| features-categories | PASSED |
| features-duplicates | PASSED |
| process-audio-basic-out-of-place | PASSED |
| process-audio-basic-in-place | PASSED |
| process-audio-double-out-of-place | SKIPPED |
| process-audio-double-in-place | SKIPPED |
| process-audio-denormals | SKIPPED |
| process-sleep-constant-mask | PASSED |
| process-sleep-process-status | PASSED |
| process-note-out-of-place-basic | PASSED |
| process-note-inconsistent | PASSED |
| process-note-wildcard | PASSED |
| process-varying-sample-rates | PASSED |
| process-varying-block-sizes | PASSED |
| process-random-block-sizes | PASSED |
| process-reset-reactivate | PASSED |
| layout-audio-ports-activation | SKIPPED |
| layout-audio-ports-config | PASSED |
| layout-configurable-audio-ports | SKIPPED |
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
| state-invalid-random | CRASHED (nih-plug's own gain example fails this too: wrapper issue) |
| state-reproducibility-basic | FAILED (nih-plug's own gain example fails this too: wrapper issue) |
| state-reproducibility-binary | FAILED (nih-plug's own gain example fails this too: wrapper issue) |
| state-reproducibility-buffered | FAILED (nih-plug's own gain example fails this too: wrapper issue) |
| transport-null | PASSED |
| transport-fuzz | PASSED |
| transport-fuzz-sample-accurate | PASSED |
