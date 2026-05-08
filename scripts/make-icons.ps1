param(
    [string]$OutDir = (Join-Path $PSScriptRoot "..\src-tauri\icons")
)

Add-Type -AssemblyName System.Drawing

if (!(Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}

function New-IconBitmap {
    param([int]$Size)

    $bmp = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic

    # Squircle background with gradient
    $radius = [Math]::Round($Size * 0.22)
    $rect = New-Object System.Drawing.Rectangle 0, 0, $Size, $Size

    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $radius * 2
    $path.AddArc($rect.X, $rect.Y, $d, $d, 180, 90)
    $path.AddArc($rect.Right - $d, $rect.Y, $d, $d, 270, 90)
    $path.AddArc($rect.Right - $d, $rect.Bottom - $d, $d, $d, 0, 90)
    $path.AddArc($rect.X, $rect.Bottom - $d, $d, $d, 90, 90)
    $path.CloseFigure()

    $colorTop = [System.Drawing.Color]::FromArgb(255, 124, 58, 237)    # purple-600
    $colorBot = [System.Drawing.Color]::FromArgb(255, 67, 56, 202)     # indigo-700
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, $colorTop, $colorBot, 135.0
    $g.FillPath($brush, $path)

    # Subtle highlight in top-left
    $highlightRect = New-Object System.Drawing.Rectangle 0, 0, $Size, ([Math]::Round($Size * 0.5))
    $highlightTop = [System.Drawing.Color]::FromArgb(60, 255, 255, 255)
    $highlightBot = [System.Drawing.Color]::FromArgb(0, 255, 255, 255)
    $highlightBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $highlightRect, $highlightTop, $highlightBot, 90.0
    $g.SetClip($path)
    $g.FillRectangle($highlightBrush, $highlightRect)
    $g.ResetClip()

    # The "D" letter
    $fontSize = [float]($Size * 0.62)
    $font = New-Object System.Drawing.Font 'Segoe UI', $fontSize, ([System.Drawing.FontStyle]::Bold)
    $brushText = [System.Drawing.Brushes]::White
    $sf = New-Object System.Drawing.StringFormat
    $sf.Alignment = [System.Drawing.StringAlignment]::Center
    $sf.LineAlignment = [System.Drawing.StringAlignment]::Center

    # Slight vertical bias upward to compensate for typeface metrics
    $textRect = New-Object System.Drawing.RectangleF 0, ([float](-$Size * 0.04)), $Size, $Size
    $g.DrawString('D', $font, $brushText, $textRect, $sf)

    # Small accent dot in the bottom-right corner — the "active / live" vibe
    $dotSize = [Math]::Round($Size * 0.13)
    $dotPad = [Math]::Round($Size * 0.13)
    $dotX = $Size - $dotPad - $dotSize
    $dotY = $Size - $dotPad - $dotSize
    $dotColor = [System.Drawing.Color]::FromArgb(255, 74, 222, 128)   # green-400
    $dotRing = [System.Drawing.Color]::FromArgb(255, 255, 255, 255)
    $dotRingPad = [Math]::Round($Size * 0.025)
    $g.FillEllipse(
        (New-Object System.Drawing.SolidBrush $dotRing),
        ($dotX - $dotRingPad),
        ($dotY - $dotRingPad),
        ($dotSize + 2 * $dotRingPad),
        ($dotSize + 2 * $dotRingPad)
    )
    $g.FillEllipse((New-Object System.Drawing.SolidBrush $dotColor), $dotX, $dotY, $dotSize, $dotSize)

    $g.Dispose()
    $font.Dispose()
    $brush.Dispose()
    $highlightBrush.Dispose()
    $path.Dispose()
    return $bmp
}

function Save-Png {
    param([System.Drawing.Bitmap]$Bmp, [string]$Path)
    $Bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
}

# PNGs at the sizes Tauri expects
$pngSizes = @(
    @{ Size = 32;   Name = '32x32.png' },
    @{ Size = 128;  Name = '128x128.png' },
    @{ Size = 256;  Name = '128x128@2x.png' },
    @{ Size = 1024; Name = 'icon.png' }
)
foreach ($entry in $pngSizes) {
    $bmp = New-IconBitmap $entry.Size
    Save-Png -Bmp $bmp -Path (Join-Path $OutDir $entry.Name)
    $bmp.Dispose()
    Write-Host "wrote $($entry.Name)"
}

# Multi-resolution ICO for Windows
function New-MultiResIco {
    param([string]$Path)
    $sizes = @(16, 24, 32, 48, 64, 128, 256)
    $pngStreams = @()
    foreach ($s in $sizes) {
        $bmp = New-IconBitmap $s
        $ms = New-Object System.IO.MemoryStream
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        $pngStreams += , @{ Size = $s; Bytes = $ms.ToArray() }
        $ms.Dispose()
    }

    $count = $pngStreams.Count
    $headerSize = 6
    $entrySize = 16
    $offset = $headerSize + ($entrySize * $count)

    $fs = New-Object System.IO.FileStream $Path, 'Create'
    $bw = New-Object System.IO.BinaryWriter $fs
    $bw.Write([UInt16]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]$count)
    foreach ($p in $pngStreams) {
        $w = if ($p.Size -ge 256) { 0 } else { $p.Size }
        $h = if ($p.Size -ge 256) { 0 } else { $p.Size }
        $bw.Write([byte]$w)
        $bw.Write([byte]$h)
        $bw.Write([byte]0)
        $bw.Write([byte]0)
        $bw.Write([UInt16]1)
        $bw.Write([UInt16]32)
        $bw.Write([UInt32]$p.Bytes.Length)
        $bw.Write([UInt32]$offset)
        $offset += $p.Bytes.Length
    }
    foreach ($p in $pngStreams) { $bw.Write($p.Bytes) }
    $bw.Flush()
    $fs.Close()
}

New-MultiResIco -Path (Join-Path $OutDir 'icon.ico')
Write-Host "wrote icon.ico"

# Minimal ICNS pointing at the high-res PNG (macOS builds)
$icnsPath = Join-Path $OutDir 'icon.icns'
$pngBytes = [System.IO.File]::ReadAllBytes((Join-Path $OutDir 'icon.png'))
$fs = New-Object System.IO.FileStream $icnsPath, 'Create'
$bw = New-Object System.IO.BinaryWriter $fs
$bw.Write([byte[]](0x69, 0x63, 0x6E, 0x73))  # 'icns'
$totalLen = 8 + 8 + $pngBytes.Length
$lenBytes = [System.BitConverter]::GetBytes([UInt32]$totalLen)
[Array]::Reverse($lenBytes)
$bw.Write($lenBytes)
$bw.Write([byte[]](0x69, 0x63, 0x30, 0x39))  # 'ic09' (512x512 PNG)
$blockLen = 8 + $pngBytes.Length
$blockLenBytes = [System.BitConverter]::GetBytes([UInt32]$blockLen)
[Array]::Reverse($blockLenBytes)
$bw.Write($blockLenBytes)
$bw.Write($pngBytes)
$bw.Flush()
$fs.Close()
Write-Host "wrote icon.icns"

Write-Host "done. preview src-tauri/icons/icon.png"
