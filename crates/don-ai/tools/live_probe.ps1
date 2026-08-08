# Live confirmation of the production-AI player fields, run inside the Parallels
# guest against a running riseofnations.exe. Results are in docs/tracks/ron-ai.md §3.5.
#
#   prlctl exec "Windows 11" powershell.exe -NoProfile -EncodedCommand \
#     $(python3 -c "import base64;print(base64.b64encode(open('live_probe.ps1').read().encode('utf-16-le')).decode())")
#
# HARD LIMIT: keep each invocation to well under ~20 ReadProcessMemory calls.
# A script issuing ~90 reads against the emulated x86 process did not return in
# 5 minutes; 7 reads return in ~40 s. Split the probe across runs instead, and
# paste the pointers you learn into the next run (that is why the string chase
# below is written with a literal address).
Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public class M{[DllImport("kernel32.dll")]public static extern IntPtr OpenProcess(int a,bool b,int p);[DllImport("kernel32.dll")]public static extern bool ReadProcessMemory(IntPtr h,IntPtr a,byte[] q,int n,out int r);public static string Rd(IntPtr h,long va,int n){byte[] q=new byte[n];int r;if(!ReadProcessMemory(h,new IntPtr(va),q,n,out r))return "FAIL";return BitConverter.ToString(q).Replace("-","");}public static string W(IntPtr h,long va,int n){byte[] q=new byte[n];int r;if(!ReadProcessMemory(h,new IntPtr(va),q,n,out r))return "FAIL";return System.Text.Encoding.Unicode.GetString(q).Replace("\0","~");}}'
$p = Get-Process riseofnations
$h = [M]::OpenProcess(0x10, $false, $p.Id)
$d = ([int64]$p.MainModule.BaseAddress) - 0x400000   # ASLR is per boot, not per process
"delta=$d"

# --- pass 1: player records (player array 0x00E3A390, stride 0x6EEC, 8 slots) ---
"P0head="   + [M]::Rd($h, (0x00E3A390 + $d),                    16)  # flags0, flags1, index
"P1head="   + [M]::Rd($h, (0x00E3A390 + $d + 0x6EEC),           16)
"P0ai="     + [M]::Rd($h, (0x00E3A390 + $d + 0x788),            12)  # stage, active, step
"P1ai="     + [M]::Rd($h, (0x00E3A390 + $d + 0x6EEC + 0x788),   12)
"P1name="   + [M]::Rd($h, (0x00E3A390 + $d + 0x6EEC + 0x6EA4),  16)  # RString header
"P0cities=" + [M]::Rd($h, (0x00E3A390 + $d + 0x3F8),             8)
"P1cities=" + [M]::Rd($h, (0x00E3A390 + $d + 0x6EEC + 0x3F8),    8)

# --- pass 2 (edit the two literals from pass 1's output, then re-run) ---
# RString header is { wchar_t* buf; u32 _; u16 len; u16 cap }; chase buf twice.
#   "P1strbuf=" + [M]::Rd($h, 0x237B816C, 32)
#   "NAME1=["   + [M]::W ($h, 0x33D0F688, 24) + "]"
#   "game2B_2D="+ [M]::Rd($h, (<GamePtr> + 0x2A), 8)     # GamePtr = *[0x00C061EC]
#   "tick="     + [M]::Rd($h, (<GamePtr> + 0x550), 4)
#   "f820="     + [M]::Rd($h, (<GamePtr> + 0x820), 4)
#   "speed="    + [M]::Rd($h, <SpeedPtr>, 4)             # SpeedPtr = *[0x00C061C0]
