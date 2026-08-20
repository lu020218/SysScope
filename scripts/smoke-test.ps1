<#
.SYNOPSIS
    SysScope pre-release smoke test: verify the app actually WORKS,
    not merely that its process exists.

.DESCRIPTION
    Designed against three real failures this project has shipped, all of
    which share one trait -- process alive, window visible, product dead:

      1. Bad manifest / missing dependency -> process exits instantly (SxS)
      2. Startup race (state not managed)  -> frontend init chain breaks,
                                              panel stuck with no data
      3. Frontend assets not embedded      -> WebView loads devUrl and
                                              shows "connection refused"

    Three independent lines of evidence are checked:
      A. process + WebView2 children alive and UI thread responsive
      B. sampler thread is doing work (CPU time / memory churn)
      C. main window screenshot actually renders content

    Launches via Explorer by default (same path as a user double-click).
    That path is the one that historically exposed a race which direct
    CreateProcess launches did not.

    NOTE: this file is intentionally ASCII-only. Windows PowerShell 5.1
    reads BOM-less UTF-8 as ANSI, which mangles non-ASCII source and
    breaks parsing.

    RUN THIS FROM AN ELEVATED SHELL. The app self-elevates on launch, and a
    non-elevated shell cannot terminate the elevated instance afterwards --
    cleanup then fails silently, and the *next* run gets folded into the
    survivor by the single-instance plugin, measuring the old process while
    reporting it as the build under test. The script now refuses to continue
    in that state rather than emit misleading results.

.PARAMETER Exe
    Executable under test. Defaults to the release build output.

.PARAMETER Launch
    explorer (default, mimics double-click) | direct (CreateProcess).

.PARAMETER KeepRunning
    Leave the app running afterwards for manual inspection.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\smoke-test.ps1
    powershell -ExecutionPolicy Bypass -File scripts\smoke-test.ps1 -Launch direct -KeepRunning
#>
[CmdletBinding()]
param(
    [string]$Exe,
    [ValidateSet("explorer", "direct")][string]$Launch = "explorer",
    [switch]$KeepRunning
)

$ErrorActionPreference = "Stop"

# Resolve default exe relative to this script (PSScriptRoot is not reliably
# populated inside param() defaults on Windows PowerShell 5.1)
if (-not $Exe) {
    $root = Split-Path -Parent $MyInvocation.MyCommand.Path
    $Exe = Join-Path $root "..\src-tauri\target\release\sysscope.exe"
}
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class SmokeWin {
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr res);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  public struct RECT { public int L,T,R,B; }
  delegate bool EnumProc(IntPtr h, IntPtr lp);
  public static IntPtr Find(uint pid, string needle) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, lp) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid) {
        var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
        if (sb.ToString().Contains(needle)) { found = h; return false; }
      }
      return true; }, IntPtr.Zero);
    return found;
  }
}
"@

$script:Failures = @()

function Check($name, $ok, $detail) {
    if ($ok) { $mark = "[PASS]" } else { $mark = "[FAIL]" }
    Write-Host ("{0} {1,-26} {2}" -f $mark, $name, $detail)
    if (-not $ok) { $script:Failures += "$name : $detail" }
}

function Get-Tree($rootId) {
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, WorkingSetSize
    $tree = @($rootId); $added = $true
    while ($added) {
        $added = $false
        foreach ($p in $all) {
            if ($tree -contains $p.ParentProcessId -and $tree -notcontains $p.ProcessId) {
                $tree += $p.ProcessId; $added = $true
            }
        }
    }
    @($all | Where-Object { $tree -contains $_.ProcessId })
}

function Stop-All {
    foreach ($p in @(Get-Process sysscope -ErrorAction SilentlyContinue)) {
        foreach ($proc in (Get-Tree $p.Id)) {
            Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
        }
    }
    Start-Sleep -Seconds 3
}

# ---------- 0. preflight ----------
$resolved = Resolve-Path $Exe -ErrorAction SilentlyContinue
if (-not $resolved) {
    Write-Host "[FAIL] executable not found: $Exe" -ForegroundColor Red
    Write-Host "       build first: npm run tauri build" -ForegroundColor Red
    exit 1
}
$Exe = $resolved.Path

Write-Host ""
Write-Host "=== SysScope smoke test ===" -ForegroundColor Cyan
Write-Host "exe    : $Exe"
Write-Host "launch : $Launch"
Write-Host ""

$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[WARN] shell is not elevated; app requires admin (UAC prompt may block automation)" -ForegroundColor Yellow
    Write-Host ""
}

Stop-All

# A leftover instance makes every measurement below meaningless: the single-instance
# plugin routes our launch into the survivor, our new pid exits, and the script then
# measures a tray-state process (0 webview children, blank capture) as if it were the
# build under test. Stop-Process fails silently on an elevated leftover when this shell
# is not elevated, so verify rather than assume.
$leftover = @(Get-Process sysscope -ErrorAction SilentlyContinue)
if ($leftover.Count -gt 0) {
    $ids = ($leftover | ForEach-Object { $_.Id }) -join ", "
    Write-Host "[FAIL] a SysScope instance survived cleanup (pid $ids)" -ForegroundColor Red
    if (-not $isAdmin) {
        Write-Host "       it is running elevated and this shell is not, so it cannot be killed here." -ForegroundColor Red
        Write-Host "       quit it from the tray icon, or from an elevated shell:" -ForegroundColor Red
        Write-Host "         Stop-Process -Name sysscope -Force" -ForegroundColor Red
    }
    Write-Host "       results would be measured against the old instance, so refusing to continue." -ForegroundColor Red
    exit 1
}

# ---------- 1. launch + survive ----------
$sw = [Diagnostics.Stopwatch]::StartNew()
if ($Launch -eq "explorer") {
    Start-Process explorer.exe -ArgumentList "`"$Exe`""
} else {
    Start-Process $Exe
}

$proc = $null
while ($sw.Elapsed.TotalSeconds -lt 25 -and -not $proc) {
    Start-Sleep -Milliseconds 300
    $proc = Get-Process sysscope -ErrorAction SilentlyContinue | Select-Object -First 1
}
if ($proc) {
    Check "process started" $true ("pid={0} in {1}s" -f $proc.Id, [math]::Round($sw.Elapsed.TotalSeconds, 2))
} else {
    Check "process started" $false "no process within 25s (manifest/dependency failure?)"
    Write-Host ""
    Write-Host "RESULT: FAIL" -ForegroundColor Red
    exit 1
}

Start-Sleep -Seconds 6
$origPid = $proc.Id
$alive = $null -ne (Get-Process -Id $origPid -ErrorAction SilentlyContinue)
$selfElevated = $false

if (-not $alive) {
    # Started without admin rights the app self-elevates: the first process
    # fires a UAC request and exits, then an elevated instance appears under a
    # NEW pid. Wait for that replacement before calling it a crash (a human may
    # need to confirm the UAC prompt, hence the generous window).
    Write-Host "       (first pid exited - waiting for an elevated instance...)"
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline -and -not $alive) {
        Start-Sleep -Milliseconds 500
        $cand = Get-Process sysscope -ErrorAction SilentlyContinue |
            Where-Object { $_.Id -ne $origPid } | Select-Object -First 1
        if ($cand) { $proc = $cand; $alive = $true; $selfElevated = $true }
    }
    if ($alive) { Start-Sleep -Seconds 6 }   # let it settle
}

if ($alive) {
    if ($selfElevated) {
        Check "survives startup" $true ("self-elevated to pid={0}" -f $proc.Id)
    } else {
        Check "survives startup" $true "no instant exit"
    }
} else {
    Check "survives startup" $false "exited with no elevated instance (SxS / missing export / UAC declined)"
    Write-Host ""
    Write-Host "RESULT: FAIL" -ForegroundColor Red
    exit 1
}

# ---------- 2. windows + webview2 ----------
Start-Sleep -Seconds 10
$hMain = [SmokeWin]::Find([uint32]$proc.Id, "SysScope -")
$hOsd = [SmokeWin]::Find([uint32]$proc.Id, "SysScope OSD")

if ($hMain -ne [IntPtr]::Zero) {
    Check "main window" $true ("visible={0}" -f [SmokeWin]::IsWindowVisible($hMain))
} else {
    Check "main window" $false "not found"
}
if ($hOsd -ne [IntPtr]::Zero) {
    Check "overlay window" $true ("visible={0}" -f [SmokeWin]::IsWindowVisible($hOsd))
} else {
    Check "overlay window" $false "not found"
}

$tree = Get-Tree $proc.Id
$wv = @($tree | Where-Object { $_.Name -eq "msedgewebview2.exe" })
$totalMb = [math]::Round((($tree | Measure-Object WorkingSetSize -Sum).Sum) / 1MB, 1)
Check "webview2 children" ($wv.Count -ge 2) ("{0} renderer/service procs" -f $wv.Count)
Check "process tree memory" ($totalMb -lt 800) ("{0} MB across {1} procs" -f $totalMb, $tree.Count)

if ($hMain -ne [IntPtr]::Zero) {
    $r = [IntPtr]::Zero
    $ok = [SmokeWin]::SendMessageTimeout($hMain, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 3000, [ref]$r)
    if ($ok -ne [IntPtr]::Zero) {
        Check "ui thread responsive" $true "answers WM_NULL"
    } else {
        Check "ui thread responsive" $false "no reply in 3s (UI thread blocked)"
    }
}

# ---------- 3. screenshot: does the panel render content? ----------
$shot = Join-Path $env:TEMP "sysscope_smoke.png"
if ($hMain -ne [IntPtr]::Zero) {
    # WebView2 suspends rendering when its window is fully occluded, so PrintWindow
    # would capture a blank frame if anything covers the panel. Raise it first, then
    # retry: the first frame after being uncovered can still be mid-paint.
    [SmokeWin]::ShowWindow($hMain, 9) | Out-Null   # SW_RESTORE
    [SmokeWin]::SetForegroundWindow($hMain) | Out-Null

    $colors = $null
    for ($attempt = 1; $attempt -le 4; $attempt++) {
        Start-Sleep -Milliseconds 700
        $rc = New-Object SmokeWin+RECT
        [SmokeWin]::GetWindowRect($hMain, [ref]$rc) | Out-Null
        $bmp = New-Object System.Drawing.Bitmap(($rc.R - $rc.L), ($rc.B - $rc.T))
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        # PW_RENDERFULLCONTENT = 2, required to capture WebView2 content
        [SmokeWin]::PrintWindow($hMain, $hdc, 2) | Out-Null
        $g.ReleaseHdc($hdc)
        $bmp.Save($shot, [System.Drawing.Imaging.ImageFormat]::Png)

        # Heuristic: a working panel is visually rich (charts, values, accent
        # colours). Both the "connection refused" error page and an all-blank
        # placeholder panel are nearly monochrome.
        $colors = New-Object 'System.Collections.Generic.HashSet[int]'
        for ($y = 0; $y -lt $bmp.Height; $y += 7) {
            for ($x = 0; $x -lt $bmp.Width; $x += 7) {
                $c = $bmp.GetPixel($x, $y)
                [void]$colors.Add((($c.R -shr 4) * 256) + (($c.G -shr 4) * 16) + ($c.B -shr 4))
            }
        }
        $g.Dispose(); $bmp.Dispose()
        if ($colors.Count -ge 12) { break }
    }
    Check "panel renders content" ($colors.Count -ge 12) ("{0} distinct tones after {1} attempt(s)" -f $colors.Count, $attempt)
}

# ---------- 4. sampler is actually running ----------
$m1 = (Get-Tree $proc.Id | Measure-Object WorkingSetSize -Sum).Sum
$c1 = (Get-Process -Id $proc.Id).CPU
Start-Sleep -Seconds 6
$m2 = (Get-Tree $proc.Id | Measure-Object WorkingSetSize -Sum).Sum
$c2 = (Get-Process -Id $proc.Id).CPU

$memDelta = [math]::Abs($m2 - $m1)
Check "memory churn" ($memDelta -gt 0) ("{0} KB over 6s (0 = sampler may be dead)" -f [math]::Round($memDelta / 1KB, 0))

$cpuDelta = [math]::Round($c2 - $c1, 3)
Check "cpu time grows" ($cpuDelta -gt 0) ("{0}s over 6s (0 = nothing running)" -f $cpuDelta)

# ---------- 5. wrap up ----------
if (-not $KeepRunning) {
    Stop-All
} else {
    Write-Host ""
    Write-Host "(app left running for manual inspection)" -ForegroundColor Yellow
}

Write-Host ""
if ($script:Failures.Count -eq 0) {
    Write-Host "RESULT: all checks passed" -ForegroundColor Green
    Write-Host "Review the screenshot to confirm real values are shown:"
    Write-Host "  $shot"
    Write-Host ""
    exit 0
} else {
    Write-Host ("RESULT: {0} check(s) failed" -f $script:Failures.Count) -ForegroundColor Red
    foreach ($f in $script:Failures) { Write-Host "  - $f" -ForegroundColor Red }
    Write-Host ""
    Write-Host "Screenshot for diagnosis: $shot"
    Write-Host ""
    exit 1
}
