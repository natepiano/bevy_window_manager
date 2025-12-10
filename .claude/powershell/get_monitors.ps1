Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;

public class DpiHelper {
    [DllImport("Shcore.dll")]
    public static extern int SetProcessDpiAwareness(int awareness);

    [DllImport("Shcore.dll")]
    public static extern int GetDpiForMonitor(IntPtr hmonitor, int dpiType, out uint dpiX, out uint dpiY);

    [DllImport("User32.dll")]
    public static extern IntPtr MonitorFromPoint(POINT pt, uint dwFlags);

    [DllImport("User32.dll")]
    public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr lprcClip, EnumMonitorsDelegate lpfnEnum, IntPtr dwData);

    [DllImport("User32.dll", CharSet = CharSet.Auto)]
    public static extern bool GetMonitorInfo(IntPtr hMonitor, ref MONITORINFOEX lpmi);

    public delegate bool EnumMonitorsDelegate(IntPtr hMonitor, IntPtr hdcMonitor, ref RECT lprcMonitor, IntPtr dwData);

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X; public int Y; }

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    public struct MONITORINFOEX {
        public int Size;
        public RECT Monitor;
        public RECT WorkArea;
        public uint Flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)]
        public string DeviceName;
    }
}
"@

# Set Per-Monitor DPI Awareness (2 = Per Monitor DPI Aware)
[DpiHelper]::SetProcessDpiAwareness(2) | Out-Null

$monitors = New-Object System.Collections.ArrayList

$callback = [DpiHelper+EnumMonitorsDelegate]{
    param($hMonitor, $hdcMonitor, [ref]$lprcMonitor, $dwData)

    $mi = New-Object DpiHelper+MONITORINFOEX
    $mi.Size = [System.Runtime.InteropServices.Marshal]::SizeOf($mi)
    [DpiHelper]::GetMonitorInfo($hMonitor, [ref]$mi) | Out-Null

    $dpiX = [uint32]0
    $dpiY = [uint32]0
    $result = [DpiHelper]::GetDpiForMonitor($hMonitor, 0, [ref]$dpiX, [ref]$dpiY)

    $scale = $dpiX / 96.0
    $isPrimary = ($mi.Flags -band 1) -eq 1

    Write-Host "Monitor: $($mi.DeviceName)"
    Write-Host "  Primary: $isPrimary"
    Write-Host "  Physical bounds: Left=$($mi.Monitor.Left) Top=$($mi.Monitor.Top) Right=$($mi.Monitor.Right) Bottom=$($mi.Monitor.Bottom)"
    Write-Host "  DPI: $dpiX x $dpiY (result: $result)"
    Write-Host "  Scale: $scale"
    Write-Host ""

    return $true
}

[DpiHelper]::EnumDisplayMonitors([IntPtr]::Zero, [IntPtr]::Zero, $callback, [IntPtr]::Zero) | Out-Null
