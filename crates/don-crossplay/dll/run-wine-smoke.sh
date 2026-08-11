#!/usr/bin/env bash
# Run the PE32 loader gate for the replacement CrossplayProxy.dll in a
# disposable macOS Wine prefix, and keep a receipt.
#
# Wine Devel's new-WoW64 prefix bootstrap can start copying the x86_64 fake-DLL
# tree before it installs the i386 syswow64 tree; on this host that path can
# terminate with a wineserver crash, leaving every PE32 program unable to load
# kernel32.dll. Seed both architecture trees before wineboot so bootstrap is
# fast, complete and deterministic. `wineboot --init` also returns before the
# registry is on disk — wineserver owns system.reg and flushes it
# asynchronously — so wait for the prefix to go idle and then poll for the file.
# Both lessons are inherited from ../../netsys-shim/run-gen7-wine-smoke.sh.
#
# This runs no retail executable, reads no retail process, and installs nothing
# into a game directory.

set -euo pipefail

fail() {
    printf 'FAIL: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf 'usage: %s CrossplayProxy.dll crossplay-load-smoke.exe EVIDENCE_DIR\n' "$0" >&2
    printf '  DON_CROSSPLAY_EXPECT_DLL_SHA256 / _SMOKE_SHA256 pin the inputs when set.\n' >&2
    exit 2
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

absolute_file() {
    local path="$1" directory
    directory="$(cd "$(dirname "$path")" && pwd -P)"
    printf '%s/%s\n' "$directory" "$(basename "$path")"
}

wine_z_path() {
    printf 'Z:%s\n' "${1//\//\\}"
}

[[ "$#" -eq 3 ]] || usage

dll="$(absolute_file "$1")"
smoke="$(absolute_file "$2")"
readonly dll smoke
readonly evidence_arg="$3"
readonly wine_root="${DON_WINE_ROOT:-/Applications/Wine Devel.app/Contents/Resources/wine}"
readonly wine="$wine_root/bin/wine"
readonly wineboot="$wine_root/bin/wineboot"
readonly wineserver="$wine_root/bin/wineserver"
readonly x64_modules="$wine_root/lib/wine/x86_64-windows"
readonly i386_modules="$wine_root/lib/wine/i386-windows"

[[ -f "$dll" ]] || fail "DLL not found: $dll"
[[ -f "$smoke" ]] || fail "smoke EXE not found: $smoke"
[[ -x "$wine" && -x "$wineboot" && -x "$wineserver" ]] || \
    fail "Wine Devel executables not found below $wine_root"
[[ -d "$x64_modules" && -d "$i386_modules" ]] || \
    fail "Wine Devel does not contain both new-WoW64 module trees"
command -v rsync >/dev/null 2>&1 || fail "rsync is required"

timeout_bin=""
if command -v gtimeout >/dev/null 2>&1; then
    timeout_bin="$(command -v gtimeout)"
elif command -v timeout >/dev/null 2>&1; then
    timeout_bin="$(command -v timeout)"
else
    fail "GNU timeout or gtimeout is required"
fi
readonly timeout_bin

readonly boot_timeout="${DON_WINE_BOOT_TIMEOUT:-300}"
readonly settle_timeout="${DON_WINE_SETTLE_TIMEOUT:-120}"
for budget in "$boot_timeout" "$settle_timeout"; do
    [[ "$budget" =~ ^[0-9]+$ ]] || fail "wine timeout budgets must be seconds: $budget"
done

dll_sha256="$(sha256_file "$dll")"
smoke_sha256="$(sha256_file "$smoke")"
readonly dll_sha256 smoke_sha256
if [[ -n "${DON_CROSSPLAY_EXPECT_DLL_SHA256:-}" ]]; then
    [[ "$dll_sha256" == "$DON_CROSSPLAY_EXPECT_DLL_SHA256" ]] || \
        fail "DLL hash mismatch: $dll_sha256"
fi
if [[ -n "${DON_CROSSPLAY_EXPECT_SMOKE_SHA256:-}" ]]; then
    [[ "$smoke_sha256" == "$DON_CROSSPLAY_EXPECT_SMOKE_SHA256" ]] || \
        fail "smoke EXE hash mismatch: $smoke_sha256"
fi

mkdir -p "$evidence_arg"
evidence="$(cd "$evidence_arg" && pwd -P)"
readonly evidence
readonly stdout_jsonl="$evidence/crossplay-load-smoke.stdout.jsonl"
readonly stderr_log="$evidence/crossplay-load-smoke.stderr.log"
readonly trace_log="$evidence/crossplay-proxy.trace.log"
readonly cmd_log="$evidence/wine-cmd.txt"
readonly receipt="$evidence/receipt.json"
for output in "$stdout_jsonl" "$stderr_log" "$trace_log" "$cmd_log" "$receipt"; do
    [[ ! -e "$output" ]] || fail "refusing to replace evidence: $output"
done

readonly temp_root="${TMPDIR:-/tmp}"
prefix="$(mktemp -d "$temp_root/don-crossplay-wine.XXXXXX")"
readonly prefix

cleanup() {
    env WINEPREFIX="$prefix" "$timeout_bin" 10 "$wineserver" -k >/dev/null 2>&1 || true
    if [[ "${DON_KEEP_WINE_PREFIX:-0}" == "1" ]]; then
        printf 'kept disposable Wine prefix: %s\n' "$prefix" >&2
        return
    fi
    case "$prefix" in
        "$temp_root"/don-crossplay-wine.*) rm -rf -- "$prefix" ;;
        *) printf 'refusing to remove unexpected prefix: %s\n' "$prefix" >&2 ;;
    esac
}
trap cleanup EXIT INT TERM

mkdir -p "$prefix/drive_c/windows/system32" "$prefix/drive_c/windows/syswow64"
rsync -a "$x64_modules/" "$prefix/drive_c/windows/system32/"
rsync -a "$i386_modules/" "$prefix/drive_c/windows/syswow64/"

readonly -a wine_env=(
    env
    "WINEPREFIX=$prefix"
    "WINEARCH=win64"
    "WINEDLLOVERRIDES=mscoree,mshtml="
    "WINEDEBUG=-all"
    "MVK_CONFIG_LOG_LEVEL=0"
)

"${wine_env[@]}" "$timeout_bin" --preserve-status "$boot_timeout" "$wineboot" --init
"${wine_env[@]}" "$timeout_bin" --preserve-status "$settle_timeout" "$wineserver" -w || true
for _ in $(seq "$settle_timeout"); do
    [[ -f "$prefix/system.reg" ]] && break
    sleep 1
done
[[ -f "$prefix/system.reg" ]] || \
    fail "wineserver never flushed system.reg (raise DON_WINE_BOOT_TIMEOUT/DON_WINE_SETTLE_TIMEOUT)"
grep -Fq '#arch=win64' "$prefix/system.reg" || fail "Wine did not create a win64/WoW64 prefix"
file "$prefix/drive_c/windows/system32/kernel32.dll" | grep -Fq 'PE32+' || \
    fail "system32 kernel32.dll is not x86_64"
file "$prefix/drive_c/windows/syswow64/kernel32.dll" | grep -Fq 'PE32 executable' || \
    fail "syswow64 kernel32.dll is not i386"

"${wine_env[@]}" "$timeout_bin" --preserve-status 30 "$wine" cmd /d /c ver >"$cmd_log"
grep -Fq 'Microsoft Windows' "$cmd_log" || fail "PE command preflight did not execute"

dll_wine="$(wine_z_path "$dll")"
trace_wine="$(wine_z_path "$trace_log")"
readonly dll_wine trace_wine
"${wine_env[@]}" "$timeout_bin" --preserve-status 120 "$wine" \
    "$smoke" "$dll_wine" "$trace_wine" >"$stdout_jsonl" 2>"$stderr_log"

grep -Fq '"schema":"don.crossplay-load-smoke.v1"' "$stdout_jsonl" || \
    fail "smoke did not emit the v1 schema"
grep -Fq '"status":"pass"' "$stdout_jsonl" || fail "smoke status is not pass"
grep -Fq '"null_slots":0' "$stdout_jsonl" || fail "an interface slot was null"
grep -Fq '"service_vtable_slots":58' "$stdout_jsonl" || fail "the service vtable is not 58 slots"
grep -Fq '"retail_process_modified":false' "$stdout_jsonl" || \
    fail "smoke did not retain the offline boundary"
grep -Fq '"game_directory_modified":false' "$stdout_jsonl" || \
    fail "smoke did not retain the offline boundary"
grep -Fq 'logger=constructed' "$trace_log" || fail "trace missed the ordinal 1 factory"
grep -Fq 'service=constructed' "$trace_log" || fail "trace missed the ordinal 2 factory"
grep -Fq 'seq=7 ' "$trace_log" || fail "trace is incomplete"

stdout_sha256="$(sha256_file "$stdout_jsonl")"
wine_version="$("$wine" --version)"
readonly stdout_sha256 wine_version
printf '{"schema":"don.crossplay-load-smoke-wine.v1","status":"pass","wine":"%s","pe":"PE32-i386","dll_sha256":"%s","smoke_sha256":"%s","stdout_sha256":"%s","retail_process_modified":false,"game_directory_modified":false}\n' \
    "$wine_version" "$dll_sha256" "$smoke_sha256" "$stdout_sha256" >"$receipt"

printf 'PASS: CrossplayProxy PE32 smoke\n'
printf 'receipt: %s\n' "$receipt"
printf 'trace: %s\n' "$trace_log"
