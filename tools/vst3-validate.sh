#!/bin/bash
# Validate the VST3 bundle with Steinberg's own validator, headless.
#
#   tools/vst3-validate.sh [-e]        # -e = extensive (≈540 tests)
#
# Fetches the minimum of the VST3 SDK (base, pluginterfaces, public.sdk,
# cmake; no VSTGUI) into target/vst3sdk on first use and builds only the
# `validator` target with cmake + ninja + clang. The SDK's cmake insists on
# Xcode; with only the Command Line Tools we pass XCODE_VERSION so its
# version check is skipped (nothing Xcode-specific is used for this
# target). Build the bundle first: cargo xtask bundle chromagrain-plugin --release
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="$ROOT/target/vst3sdk"
VALIDATOR="$SDK/build/bin/Release/validator"
BUNDLE="$ROOT/target/bundled/chromagrain.vst3"

if [ ! -x "$VALIDATOR" ]; then
  if [ ! -d "$SDK/.git" ]; then
    git clone -q --depth 1 https://github.com/steinbergmedia/vst3sdk.git "$SDK"
    git -C "$SDK" submodule update --init --depth 1 base pluginterfaces public.sdk cmake
  fi
  XCODE_VERSION=16 cmake -S "$SDK" -B "$SDK/build" -G Ninja -DXCODE_VERSION=16 \
    -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_BUILD_TYPE=Release \
    -DSMTG_ENABLE_VSTGUI_SUPPORT=OFF -DSMTG_ENABLE_VST3_PLUGIN_EXAMPLES=OFF -DSMTG_RUN_VST_VALIDATOR=OFF >/dev/null
  cmake --build "$SDK/build" --target validator >/dev/null
fi
[ -d "$BUNDLE" ] || { echo "no $BUNDLE: run cargo xtask bundle chromagrain-plugin --release"; exit 2; }
"$VALIDATOR" "$@" "$BUNDLE"
