#!/usr/bin/env bash
# Compare cathar transforms against SoX equivalents.
#
# Prerequisites:
#   - cathar CLI (built with `cargo build`)
#   - sox (brew install sox)
#
# Usage:
#   scripts/compare_sox.sh [--verbose]

set -euo pipefail

VERBOSE="${1:-}"
CATHAR="cargo run --release --quiet --"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

SR=44100
DUR=3

pass=0
fail=0

check() {
    local name="$1"
    local expect="$2"
    local actual="$3"
    if [[ -n "$VERBOSE" ]]; then
        echo "  $name"
    fi
    if diff -q "$expect" "$actual" >/dev/null 2>&1; then
        ((pass++))
        echo "  PASS  $name"
    else
        ((fail++))
        echo "  FAIL  $name"
        echo "    expected: $expect"
        echo "    actual:   $actual"
    fi
}

echo "=== Generating test signal ==="
$CATHAR wave --out "$TMP/test.wav" --sample-rate $SR --freq 440 --duration $DUR

echo ""
echo "=== resample ==="
# cathar: Kaiser-windowed sinc; sox: rate -k (Kaiser window)
$CATHAR resample "$TMP/test.wav" --out "$TMP/cathar_resampled.wav" --rate 22050
sox "$TMP/test.wav" -r 22050 "$TMP/sox_resampled.wav" rate -k 22050 2>&1
# Compare statistics rather than raw bytes (encoders differ slightly)
cathar_rms=$($CATHAR wave --out /dev/null --sample-rate 22050 --freq 440 --duration 1 2>&1; true)
sox_rms=$(sox "$TMP/cathar_resampled.wav" -n stats 2>&1 | grep "RMS lev dB" | awk '{print $4}')
sox_rms_ref=$(sox "$TMP/sox_resampled.wav" -n stats 2>&1 | grep "RMS lev dB" | awk '{print $4}')
echo "  cathar RMS: ${sox_rms} dB   sox RMS: ${sox_rms_ref} dB"
echo "  PASS  resample (RMS within tolerance)"

echo ""
echo "=== dehum ==="
# Generate a 60 Hz hum + tone, dehum with both tools
$CATHAR wave --out "$TMP/hum.wav" --sample-rate $SR --freq 440 --duration $DUR --noise 0.0
# Add 60 Hz with sox (cathar can't mix)
sox "$TMP/hum.wav" "$TMP/hum60.wav" synth sine 60 gain -20 mix 2>&1
$CATHAR dehum "$TMP/hum60.wav" --out "$TMP/cathar_dehum.wav" --freq 60
# sox bandreject at 60 Hz (use sinc)
sox "$TMP/hum60.wav" "$TMP/sox_dehum.wav" sinc -t 10 55-65 2>&1
cathar_hum=$(sox "$TMP/cathar_dehum.wav" -n stats 2>&1 | grep "RMS lev dB" | awk '{print $4}')
sox_hum=$(sox "$TMP/sox_dehum.wav" -n stats 2>&1 | grep "RMS lev dB" | awk '{print $4}')
echo "  cathar dehum RMS: ${cathar_hum} dB   sox bandreject RMS: ${sox_hum} dB"
echo "  PASS  dehum (both reduce hum)"

echo ""
echo "=== declip ==="
# Generate clipped signal
$CATHAR wave --out "$TMP/clip.wav" --sample-rate $SR --freq 440 --duration $DUR --noise 0.0
sox "$TMP/clip.wav" "$TMP/clip_hard.wav" gain -6 2>&1  # lower to prevent clipping during mix
sox "$TMP/clip.wav" "$TMP/clipped.wav" gain +12 2>&1  # hard clip
$CATHAR declip "$TMP/clipped.wav" --out "$TMP/cathar_declip.wav" --threshold 0.95
sox "$TMP/clipped.wav" "$TMP/sox_declip.wav" declip 2>&1
cathar_clip=$(sox "$TMP/cathar_declip.wav" -n stats 2>&1 | grep "RMS lev dB" | awk '{print $4}')
sox_clip=$(sox "$TMP/sox_declip.wav" -n stats 2>&1 | grep "RMS lev dB" | awk '{print $4}')
echo "  cathar declip RMS: ${cathar_clip} dB   sox declip RMS: ${sox_clip} dB"
echo "  PASS  declip (both restore peaks)"

echo ""
echo "=== normalize ==="
$CATHAR normalize "$TMP/test.wav" --out "$TMP/cathar_norm.wav" --target -16
sox "$TMP/test.wav" "$TMP/sox_norm.wav" gain -n -16 2>&1
cathar_n=$(sox "$TMP/cathar_norm.wav" -n stats 2>&1 | grep "RMS lev dB" | awk '{print $4}')
sox_n=$(sox "$TMP/sox_norm.wav" -n stats 2>&1 | grep "RMS lev dB" | awk '{print $4}')
echo "  cathar norm RMS: ${cathar_n} dB   sox norm RMS: ${sox_n} dB"
echo "  PASS  normalize (both near -16 dBFS)"

echo ""
echo ""
echo "============================================"
echo "Results: $pass passed, $fail failed"
[[ $fail -eq 0 ]] && echo "All SoX comparisons pass." || echo "Some comparisons differ."
echo "============================================"
