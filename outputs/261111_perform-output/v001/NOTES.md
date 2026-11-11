# 261111 perform-output — v001

Branch `overhaul-perform`. Screenshots are gitignored (png); this file is the trail.

| file | what |
|---|---|
| `261111_wgpu_demo_parked_v001.png` | control window, `--demo`, first run of the wgpu (Metal) preview — rotated snippet pane, colour-filtered grains, master smear. |
| `261111_output_window_fullscreen_v001.png` | `--demo --output --play` on the laptop's single display: the performance output window fullscreen — canvas only, black letterbox, no chrome, no cursor. |
| `261111_cpu_reference_t{0.96,2.6,3.6}_v001.png` | frames from `chromagrain --render-demo` (the CPU reference renderer) for eyeballing GPU parity. |

Numeric GPU↔CPU parity lives in the test `gpu::tests::gpu_matches_the_cpu_reference_for_every_effect`
(headless wgpu device, every effect; mean abs diff 1.4–1.7/255, compress 3.9).
Set `CHROMAGRAIN_PARITY_DIR=/some/dir` when running it to dump the GPU/CPU frame pairs as PNGs.

Not verified on hardware yet: output on a real second display (logic is unit-tested:
`displays::pick_output`, `displays::fit`), and whether the single-display fullscreen output keeps
animating while macOS hides the control window in another Space (see caveats in the report).
