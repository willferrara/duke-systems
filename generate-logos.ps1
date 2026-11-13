Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Drawing

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
$MasterPath = Join-Path $ScriptDir "duke-systems-master.jpg"

if (-not (Test-Path -LiteralPath $MasterPath)) {
    throw "Master logo image not found at: $MasterPath"
}

$Targets = @(
    @{ App = "duke-systems";  Out = (Join-Path $ScriptDir "duke-systems\src\dukesystems.png");   Mirror = $null; Label = $null },
    @{ App = "duke-launcher"; Out = (Join-Path $ScriptDir "duke-launcher\src\dukeflag.png");     Mirror = (Join-Path $ScriptDir "duke-systems\src\assets\dukeflag.png");    Label = "LAUNCHER" },
    @{ App = "duke-monitor";  Out = (Join-Path $ScriptDir "duke-monitor\src\dukemonitor.png");   Mirror = (Join-Path $ScriptDir "duke-systems\src\assets\dukemonitor.png"); Label = "MONITOR" },
    @{ App = "duke-capture";  Out = (Join-Path $ScriptDir "duke-capture\src\dukecapture.png");   Mirror = (Join-Path $ScriptDir "duke-systems\src\assets\dukecapture.png"); Label = "CAPTURE" },
    @{ App = "duke-input";    Out = (Join-Path $ScriptDir "duke-input\src\dukeinput.png");       Mirror = (Join-Path $ScriptDir "duke-systems\src\assets\dukeinput.png");   Label = "INPUT" }
)

function Sample-TopLeftBackgroundColor {
    param([System.Drawing.Bitmap]$Bitmap)

    $sumR = 0L
    $sumG = 0L
    $sumB = 0L
    $count = 0L

    $maxX = [Math]::Min(24, $Bitmap.Width - 1)
    $maxY = [Math]::Min(24, $Bitmap.Height - 1)
    for ($y = 4; $y -le $maxY; $y++) {
        for ($x = 4; $x -le $maxX; $x++) {
            $px = $Bitmap.GetPixel($x, $y)
            $sumR += $px.R
            $sumG += $px.G
            $sumB += $px.B
            $count++
        }
    }

    if ($count -eq 0) {
        return $Bitmap.GetPixel(0, 0)
    }

    $r = [int][Math]::Round($sumR / [double]$count)
    $g = [int][Math]::Round($sumG / [double]$count)
    $b = [int][Math]::Round($sumB / [double]$count)
    return [System.Drawing.Color]::FromArgb(255, $r, $g, $b)
}

function Find-ContentBounds {
    param(
        [System.Drawing.Bitmap]$Bitmap,
        [System.Drawing.Color]$BgColor
    )

    $minX = $Bitmap.Width
    $maxX = -1
    $minY = $Bitmap.Height
    $maxY = -1

    # Step by 2px for speed, then pad slightly so no antialiased edge is lost
    for ($y = 0; $y -lt $Bitmap.Height; $y += 2) {
        for ($x = 0; $x -lt $Bitmap.Width; $x += 2) {
            $px = $Bitmap.GetPixel($x, $y)
            $diff = [Math]::Abs($px.R - $BgColor.R) +
                    [Math]::Abs($px.G - $BgColor.G) +
                    [Math]::Abs($px.B - $BgColor.B)
            if ($diff -gt 28) {
                if ($x -lt $minX) { $minX = $x }
                if ($x -gt $maxX) { $maxX = $x }
                if ($y -lt $minY) { $minY = $y }
                if ($y -gt $maxY) { $maxY = $y }
            }
        }
    }

    if ($maxX -lt 0 -or $maxY -lt 0) {
        return [System.Drawing.Rectangle]::new(0, 0, $Bitmap.Width, $Bitmap.Height)
    }

    $pad = 8
    $x0 = [Math]::Max(0, $minX - $pad)
    $y0 = [Math]::Max(0, $minY - $pad)
    $x1 = [Math]::Min($Bitmap.Width - 1, $maxX + $pad)
    $y1 = [Math]::Min($Bitmap.Height - 1, $maxY + $pad)
    return [System.Drawing.Rectangle]::new($x0, $y0, $x1 - $x0 + 1, $y1 - $y0 + 1)
}

function Find-SubLabelBand {
    param(
        [System.Drawing.Bitmap]$Bitmap,
        [System.Drawing.Rectangle]$ContentRect
    )

    # Locate the horizontal gap between bold white DUKE and spaced SYSTEMS below it
    $yStart = [Math]::Max(0, $ContentRect.Top + [int]($ContentRect.Height * 0.65))
    $yEnd = [Math]::Min($Bitmap.Height - 1, $ContentRect.Bottom)

    $inDuke = $false
    $dukeBottom = -1
    $subTop = -1
    $subBottom = -1

    for ($y = $yStart; $y -le $yEnd; $y++) {
        $brightCount = 0
        $maxChannel = 0
        for ($x = $ContentRect.Left; $x -le $ContentRect.Right; $x += 2) {
            $px = $Bitmap.GetPixel($x, $y)
            $lum = [int](($px.R + $px.G + $px.B) / 3)
            if ($lum -gt 125) {
                $brightCount++
                if ($lum -gt $maxChannel) { $maxChannel = $lum }
            }
        }

        if (-not $inDuke -and $maxChannel -gt 215 -and $brightCount -gt 10) {
            $inDuke = $true
        } elseif ($inDuke -and $dukeBottom -lt 0 -and $brightCount -eq 0) {
            $dukeBottom = $y - 1
        } elseif ($dukeBottom -gt 0 -and $subTop -lt 0 -and $brightCount -gt 3) {
            $subTop = $y
        } elseif ($subTop -gt 0 -and $brightCount -gt 3) {
            $subBottom = $y
        }
    }

    if ($dukeBottom -lt 0 -or $subTop -lt 0 -or $subBottom -lt $subTop) {
        # Fallback proportions matching 2400x1792 master layout
        $dukeBottom = $ContentRect.Top + [int]($ContentRect.Height * 0.876)
        $subTop = $ContentRect.Top + [int]($ContentRect.Height * 0.920)
        $subBottom = $ContentRect.Bottom - 8
    }

    return @{
        DukeBottom = $dukeBottom
        SubTop     = $subTop
        SubBottom  = $subBottom
    }
}

function Pick-CleanSansFontFamily {
    $preferred = @("Segoe UI Variable Display", "Segoe UI", "Bahnschrift", "Arial")
    $installed = [System.Drawing.Text.InstalledFontCollection]::new()
    $names = @($installed.Families | ForEach-Object { $_.Name })
    $installed.Dispose()
    foreach ($candidate in $preferred) {
        if ($names -contains $candidate) {
            return $candidate
        }
    }
    return "SansSerif"
}

function Draw-SpacedSubLabel {
    param(
        [System.Drawing.Graphics]$Graphics,
        [string]$Text,
        [string]$FontFamilyName,
        [System.Drawing.Color]$TextColor,
        [float]$CenterX,
        [float]$TargetSpanWidth,
        [float]$TargetCapHeight,
        [float]$BaselineY
    )

    $chars = $Text.ToCharArray() | ForEach-Object { [string]$_ }
    if ($chars.Count -eq 0) { return }

    $family = [System.Drawing.FontFamily]::new($FontFamilyName)
    $sf = [System.Drawing.StringFormat]::GenericTypographic.Clone()
    $sf.FormatFlags = $sf.FormatFlags -bor [System.Drawing.StringFormatFlags]::MeasureTrailingSpaces

    # Calibrate emSize using reference glyph 'H' so exact cap height matches SYSTEMS
    $refPath = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $refPath.AddString("H", $family, [int][System.Drawing.FontStyle]::Regular, 100.0, [System.Drawing.PointF]::new(0, 0), $sf)
    $refBounds = $refPath.GetBounds()
    $refPath.Dispose()
    $emSize = [float](100.0 * ($TargetCapHeight / [Math]::Max(1.0, $refBounds.Height)))

    # Measure each glyph's exact visual path bounds at calibrated emSize
    $glyphInfos = @()
    $sumWidths = 0.0
    foreach ($ch in $chars) {
        $gp = [System.Drawing.Drawing2D.GraphicsPath]::new()
        $gp.AddString($ch, $family, [int][System.Drawing.FontStyle]::Regular, $emSize, [System.Drawing.PointF]::new(0, 0), $sf)
        $b = $gp.GetBounds()
        $glyphInfos += @{
            Char   = $ch
            Path   = $gp
            Bounds = $b
        }
        $sumWidths += $b.Width
    }

    # Match flat glyph baseline using 'E'/'H' bottom edge so round glyphs (C, O, S, U) sit naturally
    $flatRef = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $flatRef.AddString("E", $family, [int][System.Drawing.FontStyle]::Regular, $emSize, [System.Drawing.PointF]::new(0, 0), $sf)
    $flatBounds = $flatRef.GetBounds()
    $flatRef.Dispose()
    $originY = [float]($BaselineY - $flatBounds.Bottom)

    # For shorter labels like INPUT (5 chars), cap maximum gap so tracking stays cohesive with SYSTEMS
    $maxNaturalSpan = if ($chars.Count -le 5) {
        [Math]::Min($TargetSpanWidth, $sumWidths + ($chars.Count - 1) * ($TargetCapHeight * 1.48))
    } else {
        $TargetSpanWidth
    }

    $gap = if ($chars.Count -gt 1) {
        [Math]::Max(12.0, ($maxNaturalSpan - $sumWidths) / ($chars.Count - 1))
    } else {
        0.0
    }
    $totalWidth = $sumWidths + $gap * [Math]::Max(0, $chars.Count - 1)
    $cursorX = [float]($CenterX - ($totalWidth / 2.0))

    $brush = [System.Drawing.SolidBrush]::new($TextColor)
    $pen = [System.Drawing.Pen]::new($TextColor, 0.7)
    $pen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round

    foreach ($g in $glyphInfos) {
        $dx = [float]($cursorX - $g.Bounds.Left)
        $matrix = [System.Drawing.Drawing2D.Matrix]::new()
        $matrix.Translate($dx, $originY)
        $g.Path.Transform($matrix)
        $matrix.Dispose()

        $Graphics.FillPath($brush, $g.Path)
        $Graphics.DrawPath($pen, $g.Path)
        $cursorX += $g.Bounds.Width + $gap
        $g.Path.Dispose()
    }

    $pen.Dispose()
    $brush.Dispose()
    $sf.Dispose()
    $family.Dispose()
}

$masterBmp = [System.Drawing.Bitmap]::new($MasterPath)
try {
    $bgColor = Sample-TopLeftBackgroundColor -Bitmap $masterBmp
    $contentRect = Find-ContentBounds -Bitmap $masterBmp -BgColor $bgColor
    $band = Find-SubLabelBand -Bitmap $masterBmp -ContentRect $contentRect

    Write-Host ("Master size: {0}x{1}" -f $masterBmp.Width, $masterBmp.Height)
    Write-Host ("Sampled corner BG: #{0:X2}{1:X2}{2:X2}" -f $bgColor.R, $bgColor.G, $bgColor.B)
    Write-Host ("Content bounds: X={0}, Y={1}, W={2}, H={3}" -f $contentRect.X, $contentRect.Y, $contentRect.Width, $contentRect.Height)
    Write-Host ("Sub-label band (src): DukeBottom={0}, SubTop={1}, SubBottom={2}" -f $band.DukeBottom, $band.SubTop, $band.SubBottom)

    # Create normalized 1024x1024 master canvas with balanced padding (~9.5% top/bottom margin)
    $canvasSize = 1024
    $targetMaxHeight = 830.0
    $targetMaxWidth = 830.0
    $scale = [Math]::Min($targetMaxHeight / $contentRect.Height, $targetMaxWidth / $contentRect.Width)

    $destW = [float]($contentRect.Width * $scale)
    $destH = [float]($contentRect.Height * $scale)
    $destX = [float](($canvasSize - $destW) / 2.0)
    $destY = [float](($canvasSize - $destH) / 2.0)

    # Prepare clean cropped source bitmap with outer 14px background pixels normalized to exact $bgColor
    $cropMargin = 16
    $srcX = [Math]::Max(0, $contentRect.X - $cropMargin)
    $srcY = [Math]::Max(0, $contentRect.Y - $cropMargin)
    $srcR = [Math]::Min($masterBmp.Width - 1, $contentRect.Right + $cropMargin)
    $srcB = [Math]::Min($masterBmp.Height - 1, $contentRect.Bottom + $cropMargin)
    $srcW = $srcR - $srcX + 1
    $srcH = $srcB - $srcY + 1

    $cropBmp = [System.Drawing.Bitmap]::new($srcW, $srcH, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    for ($y = 0; $y -lt $srcH; $y++) {
        for ($x = 0; $x -lt $srcW; $x++) {
            $px = $masterBmp.GetPixel($srcX + $x, $srcY + $y)
            $diff = [Math]::Abs($px.R - $bgColor.R) +
                    [Math]::Abs($px.G - $bgColor.G) +
                    [Math]::Abs($px.B - $bgColor.B)
            # Only snap subtle JPEG noise in background areas; preserve pure black pug & bright elements
            if ($diff -le 12) {
                $cropBmp.SetPixel($x, $y, $bgColor)
            } else {
                $cropBmp.SetPixel($x, $y, $px)
            }
        }
    }

    $paddedDestX = [float]($destX - (($contentRect.X - $srcX) * $scale))
    $paddedDestY = [float]($destY - (($contentRect.Y - $srcY) * $scale))
    $paddedDestW = [float]($srcW * $scale)
    $paddedDestH = [float]($srcH * $scale)

    $master1024 = [System.Drawing.Bitmap]::new($canvasSize, $canvasSize, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $master1024.SetResolution(96, 96)
    $gMaster = [System.Drawing.Graphics]::FromImage($master1024)
    $gMaster.Clear($bgColor)
    $gMaster.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $gMaster.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $gMaster.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $gMaster.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $gMaster.DrawImage($cropBmp, [System.Drawing.RectangleF]::new($paddedDestX, $paddedDestY, $paddedDestW, $paddedDestH))
    $gMaster.Dispose()
    $cropBmp.Dispose()

    # Coordinates in 1024x1024 space for SYSTEMS cover-up and variant text
    $dukeBottom1024 = $destY + (($band.DukeBottom - $contentRect.Y) * $scale)
    $subTop1024 = $destY + (($band.SubTop - $contentRect.Y) * $scale)
    $subBottom1024 = $destY + (($band.SubBottom - $contentRect.Y) * $scale)
    $subCapHeight = [float]($subBottom1024 - $subTop1024)
    $coverTop = [float](($dukeBottom1024 + $subTop1024) / 2.0 + 2.0)
    $coverRect = [System.Drawing.RectangleF]::new(60.0, $coverTop, 904.0, [float]($canvasSize - $coverTop - 20.0))

    $fontFamily = Pick-CleanSansFontFamily
    $subColor = [System.Drawing.Color]::FromArgb(255, 0xC8, 0xD0, 0xDC)
    Write-Host ("Using sub-label font: {0}, capHeight={1:N1}px, baselineY={2:N1}px" -f $fontFamily, $subCapHeight, $subBottom1024)

    foreach ($target in $Targets) {
        $outDir = Split-Path -Parent $target.Out
        if (-not (Test-Path -LiteralPath $outDir)) {
            New-Item -ItemType Directory -Path $outDir -Force | Out-Null
        }

        if ($null -eq $target.Label) {
            $master1024.Save($target.Out, [System.Drawing.Imaging.ImageFormat]::Png)
            Write-Host "Generated master: $($target.Out)"
            continue
        }

        $variantBmp = [System.Drawing.Bitmap]::new($master1024)
        $variantBmp.SetResolution(96, 96)
        $g = [System.Drawing.Graphics]::FromImage($variantBmp)
        $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

        $coverBrush = [System.Drawing.SolidBrush]::new($bgColor)
        $g.FillRectangle($coverBrush, $coverRect)
        $coverBrush.Dispose()

        Draw-SpacedSubLabel `
            -Graphics $g `
            -Text $target.Label `
            -FontFamilyName $fontFamily `
            -TextColor $subColor `
            -CenterX ($canvasSize / 2.0) `
            -TargetSpanWidth $destW `
            -TargetCapHeight $subCapHeight `
            -BaselineY $subBottom1024

        $g.Dispose()
        $variantBmp.Save($target.Out, [System.Drawing.Imaging.ImageFormat]::Png)
        if ($target.Mirror) {
            $mirrorDir = Split-Path -Parent $target.Mirror
            if (-not (Test-Path -LiteralPath $mirrorDir)) {
                New-Item -ItemType Directory -Path $mirrorDir -Force | Out-Null
            }
            $variantBmp.Save($target.Mirror, [System.Drawing.Imaging.ImageFormat]::Png)
        }
        $variantBmp.Dispose()
        Write-Host "Generated variant ($($target.Label)): $($target.Out)"
    }

    $master1024.Dispose()
} finally {
    $masterBmp.Dispose()
}
