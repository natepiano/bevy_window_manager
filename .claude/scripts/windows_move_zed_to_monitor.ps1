# Moves Zed window to a target Bevy monitor (Windows)
# Usage: windows_move_zed_to_monitor.ps1 <target_index> <mon0_x> <mon0_y> <mon0_scale> <mon1_x> <mon1_y> <mon1_scale>
#
# Finds the Zed window titled "bevy_window_manager" (handles multiple Zed windows)
# Matches Bevy monitor to Windows monitor by position, then moves window there
# Positions it in the left half of the target monitor

param(
    [Parameter(Mandatory=$true, Position=0)][int]$TargetIndex,
    [Parameter(Mandatory=$true, Position=1)][int]$Mon0X,
    [Parameter(Mandatory=$true, Position=2)][int]$Mon0Y,
    [Parameter(Mandatory=$true, Position=3)][double]$Mon0Scale,
    [Parameter(Mandatory=$true, Position=4)][int]$Mon1X,
    [Parameter(Mandatory=$true, Position=5)][int]$Mon1Y,
    [Parameter(Mandatory=$true, Position=6)][double]$Mon1Scale
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;

public class Win32Window {
    [DllImport("user32.dll")]
    public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr lprcClip, MonitorEnumDelegate lpfnEnum, IntPtr dwData);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFO lpmi);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsDelegate lpEnumFunc, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);

    [DllImport("user32.dll")]
    public static extern int GetWindowTextLength(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter, int X, int Y, int cx, int cy, uint uFlags);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    public static extern bool IsZoomed(IntPtr hWnd);

    public const int SW_RESTORE = 9;

    public delegate bool MonitorEnumDelegate(IntPtr hMonitor, IntPtr hdcMonitor, ref RECT lprcMonitor, IntPtr dwData);
    public delegate bool EnumWindowsDelegate(IntPtr hWnd, IntPtr lParam);

    public const uint SWP_NOZORDER = 0x0004;
    public const uint SWP_NOACTIVATE = 0x0010;
    public const uint SWP_SHOWWINDOW = 0x0040;

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    public struct MONITORINFO {
        public int cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
    }

    public static List<RECT> Monitors = new List<RECT>();
    public static List<RECT> WorkAreas = new List<RECT>();

    public static bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdcMonitor, ref RECT lprcMonitor, IntPtr dwData) {
        MONITORINFO mi = new MONITORINFO();
        mi.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
        if (GetMonitorInfo(hMonitor, ref mi)) {
            Monitors.Add(mi.rcMonitor);
            WorkAreas.Add(mi.rcWork);
        }
        return true;
    }

    public static void EnumerateMonitors() {
        Monitors.Clear();
        WorkAreas.Clear();
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, MonitorEnumProc, IntPtr.Zero);
    }

    public static IntPtr ZedWindow = IntPtr.Zero;
    public static string TargetTitle = "bevy_window_manager";

    public static bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam) {
        if (!IsWindowVisible(hWnd)) return true;

        int length = GetWindowTextLength(hWnd);
        if (length == 0) return true;

        StringBuilder sb = new StringBuilder(length + 1);
        GetWindowText(hWnd, sb, sb.Capacity);
        string title = sb.ToString();

        // Check if this is a Zed window with our target title
        if (title.Contains(TargetTitle)) {
            uint processId;
            GetWindowThreadProcessId(hWnd, out processId);
            try {
                var process = System.Diagnostics.Process.GetProcessById((int)processId);
                if (process.ProcessName.ToLower() == "zed") {
                    ZedWindow = hWnd;
                    return false; // Stop enumeration
                }
            } catch { }
        }
        return true;
    }

    public static IntPtr FindZedWindow() {
        ZedWindow = IntPtr.Zero;
        EnumWindows(EnumWindowsProc, IntPtr.Zero);
        return ZedWindow;
    }
}
"@

# Enumerate monitors
[Win32Window]::EnumerateMonitors()
$monitors = [Win32Window]::Monitors
$workAreas = [Win32Window]::WorkAreas

if ($monitors.Count -eq 0) {
    Write-Error "ERROR: No monitors found"
    exit 1
}

# Convert target Bevy monitor position to Windows logical coordinates
if ($TargetIndex -eq 0) {
    $targetBevyLogicalX = [int]($Mon0X / $Mon0Scale)
    $targetBevyLogicalY = [int]($Mon0Y / $Mon0Scale)
} else {
    $targetBevyLogicalX = [int]($Mon1X / $Mon1Scale)
    $targetBevyLogicalY = [int]($Mon1Y / $Mon1Scale)
}

# Find the Windows monitor that matches the target Bevy monitor by position
$tolerance = 5
$winMonitorIndex = -1
for ($i = 0; $i -lt $monitors.Count; $i++) {
    $mon = $monitors[$i]
    if ([Math]::Abs($mon.Left - $targetBevyLogicalX) -le $tolerance -and
        [Math]::Abs($mon.Top - $targetBevyLogicalY) -le $tolerance) {
        $winMonitorIndex = $i
        break
    }
}

if ($winMonitorIndex -lt 0) {
    Write-Error "ERROR: Could not find Windows monitor matching Bevy monitor $TargetIndex at logical ($targetBevyLogicalX, $targetBevyLogicalY)"
    for ($i = 0; $i -lt $monitors.Count; $i++) {
        $mon = $monitors[$i]
        Write-Error "Windows Monitor $i`: ($($mon.Left), $($mon.Top))"
    }
    exit 1
}

# Find Zed window
$zedHwnd = [Win32Window]::FindZedWindow()

if ($zedHwnd -eq [IntPtr]::Zero) {
    Write-Error "ERROR: Could not find Zed window titled 'bevy_window_manager'"
    exit 1
}

# Get target monitor work area (excludes taskbar)
$workArea = $workAreas[$winMonitorIndex]

# Calculate position: left half of monitor with margin
$margin = 20

$targetX = $workArea.Left + $margin
$targetY = $workArea.Top + $margin

# Calculate size: left half width, most of work area height
$workWidth = $workArea.Right - $workArea.Left
$workHeight = $workArea.Bottom - $workArea.Top

$targetW = [int](($workWidth / 2) - ($margin * 2))
$targetH = [int]($workHeight * 0.7)

# Restore window if maximized (SetWindowPos doesn't work on maximized windows)
if ([Win32Window]::IsZoomed($zedHwnd)) {
    [Win32Window]::ShowWindow($zedHwnd, [Win32Window]::SW_RESTORE) | Out-Null
    Start-Sleep -Milliseconds 100
}

# Move and resize the Zed window
$result = [Win32Window]::SetWindowPos(
    $zedHwnd,
    [IntPtr]::Zero,
    $targetX,
    $targetY,
    $targetW,
    $targetH,
    [Win32Window]::SWP_NOZORDER -bor [Win32Window]::SWP_SHOWWINDOW
)

if (-not $result) {
    Write-Error "ERROR: Failed to move Zed window"
    exit 1
}

# Bring window to foreground
[Win32Window]::SetForegroundWindow($zedHwnd) | Out-Null

Write-Host "Moved Zed to monitor $TargetIndex at ($targetX, $targetY) size ${targetW}x${targetH}"

# Wait briefly for window to settle
Start-Sleep -Milliseconds 200

# Verify the move worked
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$detected = & "$scriptDir\windows_detect_zed_monitor.ps1" $Mon0X $Mon0Y $Mon0Scale $Mon1X $Mon1Y $Mon1Scale

if ($detected -ne $TargetIndex.ToString()) {
    Write-Error "WARNING: Zed detected on monitor $detected, expected $TargetIndex"
    exit 1
}

Write-Host "Verified Zed is on monitor $TargetIndex"
