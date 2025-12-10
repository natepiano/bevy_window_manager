# Run this script while the window is in different states
# to compare the frame bounds

Add-Type @"
using System;
using System.Runtime.InteropServices;

public class WindowFrame {
    [DllImport("Shcore.dll")]
    public static extern int SetProcessDpiAwareness(int awareness);
    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hwnd, int dwAttribute, out RECT pvAttribute, int cbAttribute);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    public static extern bool ClientToScreen(IntPtr hWnd, ref POINT lpPoint);

    [DllImport("user32.dll")]
    public static extern int GetWindowLong(IntPtr hWnd, int nIndex);

    [DllImport("user32.dll")]
    public static extern bool GetWindowPlacement(IntPtr hWnd, ref WINDOWPLACEMENT lpwndpl);

    [DllImport("user32.dll")]
    public static extern bool IsZoomed(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool IsIconic(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint dwFlags);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFO lpmi);

    [DllImport("Shcore.dll")]
    public static extern int GetDpiForMonitor(IntPtr hmonitor, int dpiType, out uint dpiX, out uint dpiY);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct WINDOWPLACEMENT {
        public int length;
        public int flags;
        public int showCmd;
        public POINT ptMinPosition;
        public POINT ptMaxPosition;
        public RECT rcNormalPosition;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    public struct MONITORINFO {
        public int cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
    }

    public const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
    public const int GWL_STYLE = -16;
    public const int GWL_EXSTYLE = -20;
    public const int WS_MAXIMIZE = 0x01000000;
    public const int WS_MINIMIZE = 0x20000000;
    public const int SW_SHOWMAXIMIZED = 3;
    public const int SW_SHOWNORMAL = 1;
    public const uint MONITOR_DEFAULTTONEAREST = 2;
}
"@

$proc = Get-Process -Name "simple_restore" -ErrorAction SilentlyContinue
if (-not $proc) {
    Write-Host "Process 'simple_restore' not found."
    exit 1
}

$hwnd = $proc.MainWindowHandle

# Set DPI awareness to get correct physical coordinates
[WindowFrame]::SetProcessDpiAwareness(2) | Out-Null

Write-Host "===== WINDOW STATE INFO ====="
Write-Host ""

# Check window state multiple ways
$style = [WindowFrame]::GetWindowLong($hwnd, [WindowFrame]::GWL_STYLE)
$exStyle = [WindowFrame]::GetWindowLong($hwnd, [WindowFrame]::GWL_EXSTYLE)
$isMaximizedStyle = ($style -band [WindowFrame]::WS_MAXIMIZE) -ne 0
$isMinimizedStyle = ($style -band [WindowFrame]::WS_MINIMIZE) -ne 0
$isZoomed = [WindowFrame]::IsZoomed($hwnd)
$isIconic = [WindowFrame]::IsIconic($hwnd)

Write-Host "Window Style: 0x$($style.ToString('X8'))"
Write-Host "Window ExStyle: 0x$($exStyle.ToString('X8'))"
Write-Host "WS_MAXIMIZE bit: $isMaximizedStyle"
Write-Host "WS_MINIMIZE bit: $isMinimizedStyle"
Write-Host "IsZoomed(): $isZoomed"
Write-Host "IsIconic(): $isIconic"
Write-Host ""

# Window placement
$placement = New-Object WindowFrame+WINDOWPLACEMENT
$placement.length = [System.Runtime.InteropServices.Marshal]::SizeOf($placement)
[WindowFrame]::GetWindowPlacement($hwnd, [ref]$placement) | Out-Null

$showCmdName = switch ($placement.showCmd) {
    1 { "SW_SHOWNORMAL" }
    2 { "SW_SHOWMINIMIZED" }
    3 { "SW_SHOWMAXIMIZED" }
    default { "Unknown ($($placement.showCmd))" }
}

Write-Host "WINDOWPLACEMENT:"
Write-Host "  showCmd: $showCmdName"
Write-Host "  flags: $($placement.flags)"
Write-Host "  ptMinPosition: ($($placement.ptMinPosition.X), $($placement.ptMinPosition.Y))"
Write-Host "  ptMaxPosition: ($($placement.ptMaxPosition.X), $($placement.ptMaxPosition.Y))"
Write-Host "  rcNormalPosition: L=$($placement.rcNormalPosition.Left) T=$($placement.rcNormalPosition.Top) R=$($placement.rcNormalPosition.Right) B=$($placement.rcNormalPosition.Bottom)"
Write-Host ""

# Monitor info
$hMonitor = [WindowFrame]::MonitorFromWindow($hwnd, [WindowFrame]::MONITOR_DEFAULTTONEAREST)
$monitorInfo = New-Object WindowFrame+MONITORINFO
$monitorInfo.cbSize = [System.Runtime.InteropServices.Marshal]::SizeOf($monitorInfo)
[WindowFrame]::GetMonitorInfo($hMonitor, [ref]$monitorInfo) | Out-Null

$dpiX = [uint32]0
$dpiY = [uint32]0
[WindowFrame]::GetDpiForMonitor($hMonitor, 0, [ref]$dpiX, [ref]$dpiY) | Out-Null
$scale = $dpiX / 96.0

Write-Host "Monitor Info:"
Write-Host "  rcMonitor: L=$($monitorInfo.rcMonitor.Left) T=$($monitorInfo.rcMonitor.Top) R=$($monitorInfo.rcMonitor.Right) B=$($monitorInfo.rcMonitor.Bottom)"
Write-Host "  rcWork: L=$($monitorInfo.rcWork.Left) T=$($monitorInfo.rcWork.Top) R=$($monitorInfo.rcWork.Right) B=$($monitorInfo.rcWork.Bottom)"
Write-Host "  DPI: $dpiX x $dpiY (scale: $scale)"
Write-Host ""

# GetWindowRect
$windowRect = New-Object WindowFrame+RECT
[WindowFrame]::GetWindowRect($hwnd, [ref]$windowRect) | Out-Null
Write-Host "GetWindowRect (outer, includes invisible border):"
Write-Host "  Left: $($windowRect.Left), Top: $($windowRect.Top)"
Write-Host "  Right: $($windowRect.Right), Bottom: $($windowRect.Bottom)"
Write-Host "  Size: $($windowRect.Right - $windowRect.Left) x $($windowRect.Bottom - $windowRect.Top)"
Write-Host "  Physical (x$scale): Left=$($windowRect.Left * $scale), Top=$($windowRect.Top * $scale)"
Write-Host ""

# DwmGetWindowAttribute EXTENDED_FRAME_BOUNDS
$extendedRect = New-Object WindowFrame+RECT
$size = [System.Runtime.InteropServices.Marshal]::SizeOf($extendedRect)
$result = [WindowFrame]::DwmGetWindowAttribute($hwnd, [WindowFrame]::DWMWA_EXTENDED_FRAME_BOUNDS, [ref]$extendedRect, $size)
Write-Host "DwmGetWindowAttribute EXTENDED_FRAME_BOUNDS (visible):"
Write-Host "  Left: $($extendedRect.Left), Top: $($extendedRect.Top)"
Write-Host "  Right: $($extendedRect.Right), Bottom: $($extendedRect.Bottom)"
Write-Host "  Size: $($extendedRect.Right - $extendedRect.Left) x $($extendedRect.Bottom - $extendedRect.Top)"
Write-Host ""

# Client area
$clientRect = New-Object WindowFrame+RECT
[WindowFrame]::GetClientRect($hwnd, [ref]$clientRect) | Out-Null
$clientTopLeft = New-Object WindowFrame+POINT
$clientTopLeft.X = 0
$clientTopLeft.Y = 0
[WindowFrame]::ClientToScreen($hwnd, [ref]$clientTopLeft) | Out-Null
Write-Host "Client area (content):"
Write-Host "  Top-left screen position: $($clientTopLeft.X), $($clientTopLeft.Y)"
Write-Host "  Size: $($clientRect.Right) x $($clientRect.Bottom)"
Write-Host ""

# Calculate invisible border
Write-Host "Invisible border (GetWindowRect - ExtendedFrame):"
Write-Host "  Left: $($extendedRect.Left - $windowRect.Left)"
Write-Host "  Top: $($extendedRect.Top - $windowRect.Top)"
Write-Host "  Right: $($windowRect.Right - $extendedRect.Right)"
Write-Host "  Bottom: $($windowRect.Bottom - $extendedRect.Bottom)"
