# chromagrain AUv3 (Apple)

An Audio Unit v3 instrument (`aumu` / subtype `chgr` / manufacturer `CrLb`):
a container app that carries an AUv3 app extension. The extension's Swift
`AUAudioUnit` subclass renders through the Rust engine's C ABI
(`crates/ffi/include/chromagrain.h`, static library `libchromagrain_ffi.a`).

```
apple/
  Shared/ChromagrainAudioUnit.swift  AUAudioUnit: buses, render block (MIDI -> note_on/off,
                                     host transport, sample-accurate block splitting), state
  Shared/Parameters.swift            AUParameterTree: GRAIN PITCH SMEAR LOOK, key, scale, sync, gain
  Shared/MediaDecoder.swift          AVFoundation decode -> PCM + RGBA -> post_source_pcm_rgba
                                     (the Apple "MediaBackend"; same limits as desktop ffmpeg)
  Extension/AudioUnitFactory.swift   principal class (AUViewController + AUAudioUnitFactory)
                                     with a placeholder SwiftUI editor
  Extension/Info.plist, *.entitlements
  App/                               container app (SwiftUI placeholder)
  Tests/Harness.swift                headless in-process test (no registration, no window)
  build-macos.sh                     no-Xcode build (Command Line Tools + cargo)
  project.yml                        XcodeGen spec for macOS + iOS (for when Xcode exists)
```

## Build (macOS, no Xcode needed)

```sh
apple/build-macos.sh          # -> target/apple/macos/chromagrain.app (arm64 + x86_64, ad-hoc signed)
apple/build-macos.sh --test   # + headless harness against an ffmpeg-generated test video
```

The harness registers the audio unit **in its own process only**
(`AUAudioUnit.registerSubclass`), instantiates it through the component
system, decodes media with AVFoundation, sends MIDI, renders, checks
parameters (macros, and key changes via the off-render-thread service) and
round-trips the state. Nothing is installed and nothing opens a window.

Realtime rules on the Swift side: the render block only calls the C ABI
(no allocation, locks or Objective-C). Parameter changes go through
`cg_instrument_post_*` / `cg_instrument_request_key_*` (atomics); key state
is built by `cg_instrument_service` on a 30 ms timer queue; decoded media is
posted from a background task; replaced media is freed off the render
thread.

## Registering with the system (only when asked)

`auval` and hosts only see an AUv3 once its app is registered. This changes
system state, so it is not done by the build. To do it later:

```sh
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
$LSREGISTER -f target/apple/macos/chromagrain.app
pluginkit -a target/apple/macos/chromagrain.app/Contents/PlugIns/ChromagrainAU.appex
auval -v aumu chgr CrLb
# undo
pluginkit -r target/apple/macos/chromagrain.app/Contents/PlugIns/ChromagrainAU.appex
$LSREGISTER -u target/apple/macos/chromagrain.app
```

## iOS

Kept ready, not built: the C ABI and the AVFoundation decoder are
platform-neutral, the editor has a UIKit branch, `project.yml` has iOS
targets and `cargo build -p chromagrain-ffi --target aarch64-apple-ios`
builds the engine. Building the app needs Xcode (iOS SDK, `xcodebuild`).
