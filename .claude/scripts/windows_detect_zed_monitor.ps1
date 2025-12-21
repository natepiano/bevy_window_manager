# Detects which monitor the Zed window is on (Windows)
# Usage: windows_detect_zed_monitor.ps1
# Outputs: "0" or "1" for the monitor index, or exits with error
#
# Finds the Zed window titled "bevy_window_manager" (handles multiple Zed windows)
# Uses Win32 APIs for monitor and window geometry

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

# Find which monitor contains the window center
for ($i = 0; $i -lt $monitors.Count; $i++) {
    $mon = $monitors[$i]
    if ($centerX -ge $mon.Left -and $centerX -lt $mon.Right -and
        $centerY -ge $mon.Top -and $centerY -lt $mon.Bottom) {
        Write-Output $i
        exit 0
    }
}

# If not found in any monitor, report error with debug info
Write-Error "ERROR: Window center at ($centerX, $centerY) not within any monitor bounds"
for ($i = 0; $i -lt $monitors.Count; $i++) {
    $mon = $monitors[$i]
    Write-Error "Monitor $i`: ($($mon.Left), $($mon.Top)) - ($($mon.Right), $($mon.Bottom))"
}
exit 1
