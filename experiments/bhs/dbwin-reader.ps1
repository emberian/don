# dbwin-reader.ps1 -- capture the BHS print()/print_line() channel.
#
# BHS print(v) and print_line(v) compile to an unconditional OutputDebugStringW
# call (verified at 0x00a048d0 / 0x00a04950 -- no debug-build gate, no flag).
# Windows funnels OutputDebugString through a shared 4 KB section named
# DBWIN_BUFFER plus two events. This script owns that section and tails it.
#
# RUN IT IN EMBER'S INTERACTIVE SESSION -- NOT via `prlctl exec`.
#   `prlctl exec` runs as SYSTEM in session 0. The game runs in the interactive
#   session, and OutputDebugString's objects are per-session
#   (\Sessions\<n>\BaseNamedObjects\DBWIN_BUFFER). A session-0 reader sees
#   nothing. If you need a SYSTEM-readable channel, use the XML file channel
#   (open_file/file_write_attrib/close_file) instead.
#
# Only one process may own DBWIN_BUFFER. Close DebugView before running this,
# and vice versa.
#
# Usage (in a normal PowerShell window inside the VM):
#   powershell -ExecutionPolicy Bypass -File C:\Users\ember\don\dbwin-reader.ps1
#   powershell -ExecutionPolicy Bypass -File ...\dbwin-reader.ps1 -Filter DONP
#   powershell -ExecutionPolicy Bypass -File ...\dbwin-reader.ps1 -Out C:\Users\ember\don\probe.log
#
# Ctrl-C to stop.

[CmdletBinding()]
param(
    [string]$Filter = '',
    [string]$Out    = ''
)

Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class DbWin
{
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr CreateEventW(IntPtr attrs, bool manualReset, bool initialState, string name);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr CreateFileMappingW(IntPtr hFile, IntPtr attrs, uint protect,
                                            uint maxHigh, uint maxLow, string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern IntPtr MapViewOfFile(IntPtr hMap, uint access, uint offHigh, uint offLow, UIntPtr bytes);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetEvent(IntPtr h);

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint WaitForSingleObject(IntPtr h, uint ms);

    const uint PAGE_READWRITE   = 0x04;
    const uint FILE_MAP_READ    = 0x0004;
    const uint WAIT_OBJECT_0    = 0x00000000;
    const uint WAIT_TIMEOUT     = 0x00000102;

    static IntPtr hBufferReady, hDataReady, hMap, pView;

    // Returns null on success, else an error string.
    public static string Open()
    {
        hBufferReady = CreateEventW(IntPtr.Zero, false, false, "DBWIN_BUFFER_READY");
        if (hBufferReady == IntPtr.Zero)
            return "CreateEvent(DBWIN_BUFFER_READY) failed: " + Marshal.GetLastWin32Error();

        hDataReady = CreateEventW(IntPtr.Zero, false, false, "DBWIN_DATA_READY");
        if (hDataReady == IntPtr.Zero)
            return "CreateEvent(DBWIN_DATA_READY) failed: " + Marshal.GetLastWin32Error();

        hMap = CreateFileMappingW(new IntPtr(-1), IntPtr.Zero, PAGE_READWRITE, 0, 4096, "DBWIN_BUFFER");
        if (hMap == IntPtr.Zero)
            return "CreateFileMapping(DBWIN_BUFFER) failed: " + Marshal.GetLastWin32Error();

        pView = MapViewOfFile(hMap, FILE_MAP_READ, 0, 0, new UIntPtr(4096));
        if (pView == IntPtr.Zero)
            return "MapViewOfFile failed: " + Marshal.GetLastWin32Error();

        return null;
    }

    // Blocks up to timeoutMs. Returns null on timeout, else "<pid>\t<text>".
    public static string Read(int timeoutMs)
    {
        SetEvent(hBufferReady);
        uint r = WaitForSingleObject(hDataReady, (uint)timeoutMs);
        if (r != WAIT_OBJECT_0) return null;

        int pid = Marshal.ReadInt32(pView);

        // The payload is a NUL-terminated ANSI string after the leading DWORD.
        // OutputDebugStringW is converted to ANSI by the OS before it lands here.
        byte[] raw = new byte[4092];
        Marshal.Copy(new IntPtr(pView.ToInt64() + 4), raw, 0, raw.Length);
        int len = 0;
        while (len < raw.Length && raw[len] != 0) len++;

        return pid.ToString() + "\t" + Encoding.Default.GetString(raw, 0, len);
    }
}
'@

$err = [DbWin]::Open()
if ($err) {
    Write-Error "Could not own the debug channel: $err"
    Write-Error "Most likely another debug listener (DebugView, a debugger) already holds DBWIN_BUFFER."
    exit 1
}

Write-Host "dbwin-reader: listening. Ctrl-C to stop." -ForegroundColor Green
if ($Filter) { Write-Host "dbwin-reader: filtering on '$Filter'" -ForegroundColor Green }
if ($Out)    { Write-Host "dbwin-reader: appending to $Out" -ForegroundColor Green }

while ($true) {
    $line = [DbWin]::Read(500)
    if ($null -eq $line) { continue }

    $parts = $line -split "`t", 2
    $pid_  = $parts[0]
    $text  = $parts[1].TrimEnd("`r", "`n")

    if ($Filter -and ($text -notlike "*$Filter*")) { continue }

    $stamp = (Get-Date).ToString('HH:mm:ss.fff')
    $rec   = "$stamp [$pid_] $text"

    Write-Host $rec
    if ($Out) { Add-Content -LiteralPath $Out -Value $rec -Encoding UTF8 }
}
