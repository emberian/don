#!/bin/sh
# Build the RoNtoy overlay and prove, from the linked binary, that it kept the
# read-only promise. Exits non-zero if any forbidden symbol appears.
#
#   tools/rontoy-overlay/build.sh [output-path]
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
out=${1:-"$root/target/rontoy-overlay"}
mkdir -p "$(dirname "$out")"

swiftc -O "$root/tools/rontoy-overlay/RoNtoyOverlay.swift" -o "$out"
echo "built $out"

# Input synthesis, accessibility control, foreign-process memory, and screen capture.
# The overlay must contain none of them. CGWindowListCopyWindowInfo (window bounds,
# no pixels, no TCC prompt) is allowed and is how the card follows the VM window.
forbidden='CGEventPost|CGEventTapCreate|CGPostKeyboardEvent|CGWarpMouseCursorPosition|AXUIElement|task_for_pid|mach_vm_write|mach_vm_read|vm_write|CGWindowListCreateImage|CGDisplayStream|SCStreamConfiguration|SCShareableContent|CGDisplayCapture'
if nm -u "$out" | grep -E "$forbidden" ; then
    echo "FAIL: overlay links a forbidden write/injection/capture symbol" >&2
    exit 1
fi
echo "audit ok: no input-synthesis, accessibility-control, foreign-memory, or screen-capture symbol"
nm -u "$out" | grep -oE 'CGWindowList[A-Za-z]*|NSURLSession' | sort -u | sed 's/^/  uses: /'
