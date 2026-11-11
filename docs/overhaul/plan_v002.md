# chromagrain overhaul — plan v002 (261111)

Changed from v001: Arden doesn't use Logic. The target is now **VST3 + AUv3 that run
anywhere (macOS, Windows, iPad)**, plus a standalone app. AUv3 moves from optional to first-class.

## Architecture
```
chromagrain-core     Rust, platform-free engine (grains, fx, sequencer, plyphon SC).
                     Media decoding behind a trait: ffmpeg on desktop, AVFoundation on iOS.
Instrument facade    process(block), MIDI note on/off, 4 macros (GRAIN/PITCH/SMEAR/LOOK),
                     host tempo/transport sync, video frame-state query. C ABI via cbindgen.
Renderer             glow -> wgpu (Metal on Mac/iPad, D3D12/Vulkan on Windows).
UI                   One design everywhere: egui + wgpu, custom-painted, touch-first.
Desktop plugin       nih-plug -> VST3 (+ CLAP) on Mac/Windows/Linux, plus standalone app.
AUv3                 Xcode project: Swift AUAudioUnit + Metal view calling the Rust
                     staticlib/xcframework (iOS + macOS).
Perform              Fullscreen output window / second display, cursorless; Syphon (mac) later.
```

## Phases
1. Design spec (HTML, design/v001): Play / Sequence / Perform; touch-first; responsive 960x600 -> 1600x1000.
2. Instrument facade + tests (in progress).
3. glow -> wgpu port; performance output window.
4. New UI in egui per the design spec (classic UI behind --classic until parity).
5. VST3 via nih-plug (Mac + Windows CI build).
6. AUv3 shell (macOS first, then iPad; iOS media decode via AVFoundation).
