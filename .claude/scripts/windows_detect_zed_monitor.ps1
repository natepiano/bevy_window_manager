# Detects which monitor the Zed window is on (Windows)
# Usage: windows_detect_zed_monitor.ps1 <mon0_x> <mon0_y> <mon0_scale> <mon1_x> <mon1_y> <mon1_scale>
# Outputs: "0" or "1" for the Bevy monitor index, or exits with error
#
# Finds the Zed window titled "bevy_window_manager" (handles multiple Zed windows)
# Matches Windows monitor to Bevy monitor by comparing positions (accounting for scale)

param(
    [Parameter(Mandatory=$true, Position=0)][int]$Mon0X,
    [Parameter(Mandatory=$true, Position=1)][int]$Mon0Y,
    [Parameter(Mandatory=$true, Position=2)][double]$Mon0Scale,
    [Parameter(Mandatory=$true, Position=3)][int]$Mon1X,
    [Parameter(Mandatory=$true, Position=4)][int]$Mon1Y,
    [Parameter(Mandatory=$true, Position=5)][double]$Mon1Scale
)

$ErrorActionPreference = "Stop"

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Collections.Generic;

public class Win32Monitor {
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
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    public delegate bool MonitorEnumDelegate(IntPtr hMonitor, IntPtr hdcMonitor, ref RECT lprcMonitor, IntPtr dwData);
    public delegate bool EnumWindowsDelegate(IntPtr hWnd, IntPtr lParam);

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

    public static bool MonitorEnumProc(IntPtr hMonitor, IntPtr hdcMonitor, ref RECT lprcMonitor, IntPtr dwData) {
        MONITORINFO mi = new MONITORINFO();
        mi.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
        if (GetMonitorInfo(hMonitor, ref mi)) {
            Monitors.Add(mi.rcMonitor);
        }
        return true;
    }

    public static void EnumerateMonitors() {
        Monitors.Clear();
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
[Win32Monitor]::EnumerateMonitors()
$monitors = [Win32Monitor]::Monitors

if ($monitors.Count -eq 0) {
    Write-Error "ERROR: No monitors found"
    exit 1
}

# Find Zed window
$zedHwnd = [Win32Monitor]::FindZedWindow()

if ($zedHwnd -eq [IntPtr]::Zero) {
    Write-Error "ERROR: Could not find Zed window titled 'bevy_window_manager'"
    exit 1
}

# Get Zed window position
$rect = New-Object Win32Monitor+RECT
if (-not [Win32Monitor]::GetWindowRect($zedHwnd, [ref]$rect)) {
    Write-Error "ERROR: Could not get Zed window position"
    exit 1
}

# Use center of window for monitor detection (matches Rust behavior)
$centerX = [int](($rect.Left + $rect.Right) / 2)
$centerY = [int](($rect.Top + $rect.Bottom) / 2)

# Find which Windows monitor contains the window center
$containingMonitor = $null
for ($i = 0; $i -lt $monitors.Count; $i++) {
    $mon = $monitors[$i]
    if ($centerX -ge $mon.Left -and $centerX -lt $mon.Right -and
        $centerY -ge $mon.Top -and $centerY -lt $mon.Bottom) {
        $containingMonitor = $mon
        break
    }
}

if ($null -eq $containingMonitor) {
    Write-Error "ERROR: Window center at ($centerX, $centerY) not within any monitor bounds"
    for ($i = 0; $i -lt $monitors.Count; $i++) {
        $mon = $monitors[$i]
        Write-Error "Monitor $i`: ($($mon.Left), $($mon.Top)) - ($($mon.Right), $($mon.Bottom))"
    }
    exit 1
}

# Convert Bevy physical positions to Windows logical positions for matching
$bevy0LogicalX = [int]($Mon0X / $Mon0Scale)
$bevy0LogicalY = [int]($Mon0Y / $Mon0Scale)
$bevy1LogicalX = [int]($Mon1X / $Mon1Scale)
$bevy1LogicalY = [int]($Mon1Y / $Mon1Scale)

# Match the containing Windows monitor to Bevy monitor by position
# Use a tolerance of 5 pixels for floating point scale factor rounding
$tolerance = 5
$winMonX = $containingMonitor.Left
$winMonY = $containingMonitor.Top

if ([Math]::Abs($winMonX - $bevy0LogicalX) -le $tolerance -and
    [Math]::Abs($winMonY - $bevy0LogicalY) -le $tolerance) {
    Write-Output 0
    exit 0
}

if ([Math]::Abs($winMonX - $bevy1LogicalX) -le $tolerance -and
    [Math]::Abs($winMonY - $bevy1LogicalY) -le $tolerance) {
    Write-Output 1
    exit 0
}

# No match found - report debug info
Write-Error "ERROR: Could not match Windows monitor at ($winMonX, $winMonY) to any Bevy monitor"
Write-Error "Bevy Monitor 0 logical: ($bevy0LogicalX, $bevy0LogicalY)"
Write-Error "Bevy Monitor 1 logical: ($bevy1LogicalX, $bevy1LogicalY)"
exit 1
