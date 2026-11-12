#!/bin/bash
# Build the macOS AUv3 (container app + 'aumu' app extension) WITHOUT Xcode:
# just the Command Line Tools (swiftc, clang, lipo, codesign) and cargo.
#
#   apple/build-macos.sh            # build target/apple/macos/chromagrain.app (universal, ad-hoc signed)
#   apple/build-macos.sh --test     # ...and run the headless in-process harness
#
# Nothing is installed or registered: the .app stays in target/. To try it
# in a host, the app has to be registered with the system first (ask before
# doing that; see apple/README.md). When Xcode is available, apple/project.yml
# (XcodeGen) describes the same targets for macOS and iOS.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APPLE="$ROOT/apple"
OUT="$ROOT/target/apple/macos"
WORK="$OUT/work"
APP="$OUT/chromagrain.app"
APPEX="$APP/Contents/PlugIns/ChromagrainAU.appex"
MIN_MACOS=12.0
ARCHS=(arm64 x86_64)
HEADER="$ROOT/crates/ffi/include/chromagrain.h"
SDK="$(xcrun --sdk macosx --show-sdk-path)"
SWIFT_FLAGS=(-sdk "$SDK" -swift-version 5 -O -import-objc-header "$HEADER")
LIBS=(-liconv -framework AudioToolbox -framework AVFoundation -framework CoreAudioKit -framework CoreMedia
      -framework CoreVideo -framework Accelerate -framework SwiftUI -framework AppKit)

rust_triple() { [ "$1" = arm64 ] && echo aarch64-apple-darwin || echo x86_64-apple-darwin; }

echo "==> Rust engine (staticlib) for ${ARCHS[*]}"
for arch in "${ARCHS[@]}"; do
  MACOSX_DEPLOYMENT_TARGET=$MIN_MACOS cargo build --manifest-path "$ROOT/Cargo.toml" -p chromagrain-ffi --release --target "$(rust_triple "$arch")" --quiet
done

rm -rf "$APP" "$WORK"
mkdir -p "$WORK" "$APP/Contents/MacOS" "$APPEX/Contents/MacOS"

echo "==> Swift: extension, container app"
for arch in "${ARCHS[@]}"; do
  lib="$ROOT/target/$(rust_triple "$arch")/release"
  swiftc "${SWIFT_FLAGS[@]}" -target "$arch-apple-macos$MIN_MACOS" -module-name ChromagrainAU \
    -application-extension -parse-as-library \
    "$APPLE"/Shared/*.swift "$APPLE"/Extension/*.swift \
    -L "$lib" -lchromagrain_ffi "${LIBS[@]}" \
    -Xlinker -e -Xlinker _NSExtensionMain \
    -o "$WORK/ChromagrainAU-$arch"
  swiftc -sdk "$SDK" -swift-version 5 -O -target "$arch-apple-macos$MIN_MACOS" -module-name Chromagrain \
    -parse-as-library "$APPLE"/App/*.swift -framework SwiftUI -framework AppKit \
    -o "$WORK/Chromagrain-$arch"
done
lipo -create "$WORK"/ChromagrainAU-* -output "$APPEX/Contents/MacOS/ChromagrainAU"
lipo -create "$WORK"/Chromagrain-* -output "$APP/Contents/MacOS/Chromagrain"

echo "==> Bundles"
cp "$APPLE/Extension/Info.plist" "$APPEX/Contents/Info.plist"
cp "$APPLE/App/Info.plist" "$APP/Contents/Info.plist"
printf 'XPC!????' > "$APPEX/Contents/PkgInfo"
printf 'APPL????' > "$APP/Contents/PkgInfo"
plutil -lint -s "$APPEX/Contents/Info.plist" "$APP/Contents/Info.plist"

echo "==> Ad-hoc signing (extension first, then the app)"
codesign --force --sign - --timestamp=none --entitlements "$APPLE/Extension/Extension.entitlements" "$APPEX"
codesign --force --sign - --timestamp=none --entitlements "$APPLE/App/App.entitlements" "$APP"
codesign --verify --deep --strict "$APP"
echo "built $APP"
lipo -info "$APPEX/Contents/MacOS/ChromagrainAU"

if [ "${1:-}" = "--test" ]; then
  echo "==> Headless harness (in-process registration only)"
  arch=$(uname -m)
  lib="$ROOT/target/$(rust_triple "$arch")/release"
  swiftc "${SWIFT_FLAGS[@]}" -target "$arch-apple-macos$MIN_MACOS" -module-name ChromagrainHarness -parse-as-library \
    "$APPLE"/Shared/*.swift "$APPLE"/Tests/Harness.swift \
    -L "$lib" -lchromagrain_ffi "${LIBS[@]}" -o "$WORK/harness"
  media="$WORK/test-media.mp4"
  if command -v ffmpeg >/dev/null; then
    ffmpeg -v error -y -f lavfi -i "testsrc2=size=640x360:rate=30:duration=3" \
      -f lavfi -i "sine=frequency=220:duration=3:sample_rate=44100" \
      -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest "$media"
  else
    echo "ffmpeg not found: using a generated .wav"
    afconvert -f WAVE -d LEI16 /System/Library/Sounds/Ping.aiff "$WORK/test-media.wav"
    media="$WORK/test-media.wav"
  fi
  "$WORK/harness" "$media"
fi
