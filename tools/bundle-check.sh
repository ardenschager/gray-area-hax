#!/bin/bash
# Build the VST3 + CLAP bundles and check them, with no host and no window.
#
#   tools/bundle-check.sh             # bundle (release) + structural checks
#   tools/bundle-check.sh --validate  # ... + clap-validator and Steinberg's
#                                     #     validator when available (both load
#                                     #     the plugin in-process, headless)
#   tools/bundle-check.sh --no-build  # check an existing target/bundled
#
# The Steinberg validator runs when VST3_VALIDATOR=/path/to/validator is set,
# or target/vst3sdk has one (tools/vst3-validate.sh builds it; set
# BUILD_VST3_VALIDATOR=1 to let this script fetch + build it).
#
# Checks: bundle layout, Info.plist (macOS), the binary's architecture, and
# that the plugin entry points are exported (VST3 GetPluginFactory [+
# bundleEntry/bundleExit on macOS], CLAP clap_entry). Exit 0 = all good.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BUILD=1
VALIDATE=0
for a in "$@"; do
  case "$a" in
    --validate) VALIDATE=1 ;;
    --no-build) BUILD=0 ;;
    *) echo "unknown option $a"; exit 2 ;;
  esac
done

fail=0
ok() { printf '  ok    %s\n' "$1"; }
bad() { printf '  FAIL  %s\n' "$1"; fail=1; }
check() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }

if [ "$BUILD" = 1 ]; then
  echo "== cargo xtask bundle chromagrain-plugin --release"
  cargo xtask bundle chromagrain-plugin --release 2>&1 | tail -3
fi

OUT="$ROOT/target/bundled"
VST3="$OUT/chromagrain.vst3"
CLAP="$OUT/chromagrain.clap"
echo "== bundles in $OUT"
case "$(uname -s)" in
  Darwin)
    VST3_BIN="$VST3/Contents/MacOS/chromagrain"
    CLAP_BIN="$CLAP/Contents/MacOS/chromagrain"
    check "VST3 bundle binary" "[ -f '$VST3_BIN' ]"
    check "CLAP bundle binary" "[ -f '$CLAP_BIN' ]"
    for b in "$VST3" "$CLAP"; do
      plist="$b/Contents/Info.plist"
      check "$(basename "$b") Info.plist is valid" "plutil -lint -s '$plist'"
      check "$(basename "$b") has a bundle id" "/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' '$plist' >/dev/null 2>&1"
    done
    check "VST3 binary is a Mach-O library" "file '$VST3_BIN' | grep -Eq 'Mach-O.*(bundle|shared library)'"
    echo "  arch  $(lipo -archs "$VST3_BIN" 2>/dev/null || echo '?')"
    syms() { nm -gU "$1" 2>/dev/null; }
    check "VST3 exports GetPluginFactory" "syms '$VST3_BIN' | grep -q ' _GetPluginFactory$'"
    check "VST3 exports bundleEntry/bundleExit" "syms '$VST3_BIN' | grep -q ' _bundleEntry$' && syms '$VST3_BIN' | grep -q ' _bundleExit$'"
    check "CLAP exports clap_entry" "syms '$CLAP_BIN' | grep -q ' _clap_entry$'"
    ;;
  Linux)
    VST3_BIN="$VST3/Contents/x86_64-linux/chromagrain.so"
    CLAP_BIN="$CLAP"
    check "VST3 bundle binary" "[ -f '$VST3_BIN' ]"
    check "CLAP binary" "[ -f '$CLAP_BIN' ]"
    check "VST3 exports GetPluginFactory" "nm -D --defined-only '$VST3_BIN' | grep -q ' GetPluginFactory$'"
    check "CLAP exports clap_entry" "nm -D --defined-only '$CLAP_BIN' | grep -q ' clap_entry$'"
    ;;
  *)
    VST3_BIN="$VST3/Contents/x86_64-win/chromagrain.vst3"
    check "VST3 bundle binary" "[ -f '$VST3_BIN' ]"
    check "CLAP binary" "[ -f '$CLAP' ]"
    ;;
esac

if [ "$VALIDATE" = 1 ]; then
  echo "== validators (in-process, headless)"
  if command -v clap-validator >/dev/null; then
    if clap-validator validate "$CLAP" >"$OUT/clap-validator.log" 2>&1; then
      ok "clap-validator: $(grep -Eo '[0-9]+ tests run.*' "$OUT/clap-validator.log" | tail -1)"
    else
      bad "clap-validator (see $OUT/clap-validator.log)"
    fi
  else
    echo "  skip  clap-validator not on PATH"
  fi
  if [ -n "${VST3_VALIDATOR:-}" ]; then
    if "$VST3_VALIDATOR" "$VST3" >"$OUT/vst3-validator.log" 2>&1; then
      ok "VST3 validator: $(grep -Eo 'Result: .*' "$OUT/vst3-validator.log" | tail -1)"
    else
      bad "VST3 validator (see $OUT/vst3-validator.log)"
    fi
  elif [ -x "$ROOT/target/vst3sdk/build/bin/Release/validator" ] || [ "${BUILD_VST3_VALIDATOR:-0}" = 1 ]; then
    if "$ROOT/tools/vst3-validate.sh" >"$OUT/vst3-validator.log" 2>&1; then
      ok "VST3 validator: $(grep -Eo 'Result: .*' "$OUT/vst3-validator.log" | tail -1)"
    else
      bad "VST3 validator (see $OUT/vst3-validator.log)"
    fi
  else
    echo "  skip  Steinberg validator not built (BUILD_VST3_VALIDATOR=1 to fetch + build it)"
  fi
fi

if [ "$fail" = 0 ]; then echo "bundle check: OK"; else echo "bundle check: FAILED"; fi
exit "$fail"
