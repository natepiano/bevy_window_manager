Add-Type @"
using System;
using System.Runtime.InteropServices;

public class WindowFrame {
    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hwnd, int dwAttribute, out RECT pvAttribute, int cbAttribute);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    public static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    public static extern bool ClientToScreen(IntPtr hWnd, ref POINT lpPoint);

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

    public const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
}
"@

# Find the window by process name
$proc = Get-Process -Name "simple_restore" -ErrorAction SilentlyContinue
if (-not $proc) {
    Write-Host "Process 'simple_restore' not found."
    exit 1
}

$hwnd = $proc.MainWindowHandle

if ($hwnd -eq [IntPtr]::Zero) {
    Write-Host "Window handle is null"
    exit 1
}

Write-Host "Found window handle: $hwnd"
Write-Host "Window title: $($proc.MainWindowTitle)"
Write-Host ""

# GetWindowRect - includes the invisible border
$windowRect = New-Object WindowFrame+RECT
[WindowFrame]::GetWindowRect($hwnd, [ref]$windowRect) | Out-Null
Write-Host "GetWindowRect (includes invisible frame):"
Write-Host "  Left: $($windowRect.Left), Top: $($windowRect.Top)"
Write-Host "  Right: $($windowRect.Right), Bottom: $($windowRect.Bottom)"
Write-Host "  Size: $($windowRect.Right - $windowRect.Left) x $($windowRect.Bottom - $windowRect.Top)"
Write-Host ""

# DwmGetWindowAttribute EXTENDED_FRAME_BOUNDS - the visible area
$extendedRect = New-Object WindowFrame+RECT
$size = [System.Runtime.InteropServices.Marshal]::SizeOf($extendedRect)
$result = [WindowFrame]::DwmGetWindowAttribute($hwnd, [WindowFrame]::DWMWA_EXTENDED_FRAME_BOUNDS, [ref]$extendedRect, $size)
Write-Host "DwmGetWindowAttribute EXTENDED_FRAME_BOUNDS (visible frame):"
Write-Host "  Result: $result (0 = success)"
Write-Host "  Left: $($extendedRect.Left), Top: $($extendedRect.Top)"
Write-Host "  Right: $($extendedRect.Right), Bottom: $($extendedRect.Bottom)"
Write-Host "  Size: $($extendedRect.Right - $extendedRect.Left) x $($extendedRect.Bottom - $extendedRect.Top)"
Write-Host ""

# GetClientRect + ClientToScreen - the content area
$clientRect = New-Object WindowFrame+RECT
[WindowFrame]::GetClientRect($hwnd, [ref]$clientRect) | Out-Null
$clientTopLeft = New-Object WindowFrame+POINT
$clientTopLeft.X = 0
$clientTopLeft.Y = 0
[WindowFrame]::ClientToScreen($hwnd, [ref]$clientTopLeft) | Out-Null
Write-Host "Client area (content, no decorations):"
Write-Host "  Top-left screen position: $($clientTopLeft.X), $($clientTopLeft.Y)"
Write-Host "  Size: $($clientRect.Right) x $($clientRect.Bottom)"
Write-Host ""

# Calculate the differences
Write-Host "Frame offsets:"
Write-Host "  Invisible border (WindowRect vs ExtendedFrame):"
Write-Host "    Left: $($extendedRect.Left - $windowRect.Left)"
Write-Host "    Top: $($extendedRect.Top - $windowRect.Top)"
Write-Host "    Right: $($windowRect.Right - $extendedRect.Right)"
Write-Host "    Bottom: $($windowRect.Bottom - $extendedRect.Bottom)"
