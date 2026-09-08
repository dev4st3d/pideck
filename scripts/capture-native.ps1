# Captures the existing native application's client area. Does not launch Pi,
# fabricate content, resize the bitmap, or overwrite a design reference.
param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][string]$Output
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Native capture requires Windows.' }
Add-Type -AssemblyName System.Drawing
if (-not ('PiDeckNativeCapture' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class PiDeckNativeCapture {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
}
'@
}
$process = Get-Process -Id $ProcessId
$process.Refresh()
$handle = $process.MainWindowHandle
if ($handle -eq [IntPtr]::Zero) { throw "Process $ProcessId has no main window." }
$destination = [IO.Path]::GetFullPath($Output)
if ([IO.Path]::GetExtension($destination) -ne '.png') { throw 'Output must be a PNG path.' }
if (Test-Path $destination) { throw "Refusing to overwrite $destination." }
$previousDpi = [PiDeckNativeCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))
$bitmap = $null; $graphics = $null
try {
    [PiDeckNativeCapture]::ShowWindow($handle, 9) | Out-Null
    if (-not [PiDeckNativeCapture]::SetForegroundWindow($handle)) {
        throw 'Could not bring the window forward. Activate PiDeck, then capture again.'
    }
    Start-Sleep -Milliseconds 350
    $rect = New-Object PiDeckNativeCapture+RECT
    $origin = New-Object PiDeckNativeCapture+POINT
    if (-not [PiDeckNativeCapture]::GetClientRect($handle, [ref]$rect) -or
        -not [PiDeckNativeCapture]::ClientToScreen($handle, [ref]$origin)) { throw 'Could not measure native client area.' }
    $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
    if ($width -ne 1440 -or $height -ne 960) {
        throw "Native client is ${width}x${height}, not 1440x960. Set Windows scaling to 100% and use the default unmaximised PiDeck window."
    }
    $desktopX = [PiDeckNativeCapture]::GetSystemMetrics(76)
    $desktopY = [PiDeckNativeCapture]::GetSystemMetrics(77)
    $desktopW = [PiDeckNativeCapture]::GetSystemMetrics(78)
    $desktopH = [PiDeckNativeCapture]::GetSystemMetrics(79)
    if ($origin.X -lt $desktopX -or $origin.Y -lt $desktopY -or
        $origin.X + $width -gt $desktopX + $desktopW -or $origin.Y + $height -gt $desktopY + $desktopH) {
        throw 'The native window is partly off-screen. Move it fully onto the desktop before capturing.'
    }
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
    $bitmap = New-Object Drawing.Bitmap($width, $height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($origin.X, $origin.Y, 0, 0, $bitmap.Size)
    $bitmap.Save($destination, [Drawing.Imaging.ImageFormat]::Png)
    Write-Output $destination
} finally {
    if ($null -ne $graphics) { $graphics.Dispose() }
    if ($null -ne $bitmap) { $bitmap.Dispose() }
    if ($previousDpi -ne [IntPtr]::Zero) { [PiDeckNativeCapture]::SetThreadDpiAwarenessContext($previousDpi) | Out-Null }
}
