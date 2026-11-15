#requires -Version 5.1
<#
.SYNOPSIS
  Zero-token static hygiene, DOM/SVG/IPC parity, and privacy verification for The Duke Suite.
.DESCRIPTION
  Runs fast (<2s) workspace-relative checks across duke-launcher and tracked repository files.
#>
[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Sw = [System.Diagnostics.Stopwatch]::StartNew()
$RepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$Failures = New-Object System.Collections.Generic.List[string]
$CheckResults = New-Object System.Collections.Generic.List[object]

function Add-CheckResult {
    param(
        [string]$Name,
        [bool]$Passed,
        [string]$Detail
    )
    $script:CheckResults.Add([PSCustomObject]@{
        Check  = $Name
        Status = $(if ($Passed) { 'PASS' } else { 'FAIL' })
        Detail = $Detail
    })
    if (-not $Passed) {
        $script:Failures.Add("$Name : $Detail")
    }
}

Push-Location $RepoRoot
try {
    # ------------------------------------------------------------------
    # 1. JavaScript Syntax Check (node --check)
    # ------------------------------------------------------------------
    $NodeCmd = Get-Command node -ErrorAction SilentlyContinue
    if (-not $NodeCmd) {
        Add-CheckResult 'JS Syntax (node --check)' $false 'node executable not found on PATH'
    } else {
        $JsTargets = @('duke-launcher/src/api.js', 'duke-launcher/src/virtual_grid.js', 'duke-launcher/src/main.js')
        $JsErrors = @()
        foreach ($Rel in $JsTargets) {
            $Abs = Join-Path $RepoRoot $Rel
            if (-not [System.IO.File]::Exists($Abs)) {
                $JsErrors += "Missing file: $Rel"
                continue
            }
            $Out = & node --check $Abs 2>&1
            if ($LASTEXITCODE -ne 0) {
                $JsErrors += "$Rel failed node --check: $($Out -join ' ')"
            }
        }
        Add-CheckResult 'JS Syntax (node --check)' ($JsErrors.Count -eq 0) $(if ($JsErrors.Count -eq 0) { 'api.js, virtual_grid.js & main.js valid ES modules' } else { $JsErrors -join '; ' })
    }

    # Load frontend and Rust entry files once into memory
    $HtmlPath = Join-Path $RepoRoot 'duke-launcher/src/index.html'
    $MainJsPath = Join-Path $RepoRoot 'duke-launcher/src/main.js'
    $ApiJsPath = Join-Path $RepoRoot 'duke-launcher/src/api.js'
    $VirtualGridJsPath = Join-Path $RepoRoot 'duke-launcher/src/virtual_grid.js'
    $MainRsPath = Join-Path $RepoRoot 'duke-launcher/src-tauri/src/main.rs'
    $TauriConfPath = Join-Path $RepoRoot 'duke-launcher/src-tauri/tauri.conf.json'
    $CapPath = Join-Path $RepoRoot 'duke-launcher/src-tauri/capabilities/default.json'
    $CargoTomlPath = Join-Path $RepoRoot 'duke-launcher/src-tauri/Cargo.toml'

    $HtmlText = [System.IO.File]::ReadAllText($HtmlPath)
    $MainJsText = [System.IO.File]::ReadAllText($MainJsPath)
    $ApiJsText = [System.IO.File]::ReadAllText($ApiJsPath)
    $VirtualGridJsText = if ([System.IO.File]::Exists($VirtualGridJsPath)) { [System.IO.File]::ReadAllText($VirtualGridJsPath) } else { '' }
    $MainRsText = [System.IO.File]::ReadAllText($MainRsPath)
    $CombinedJsText = $MainJsText + "`n" + $VirtualGridJsText + "`n" + $ApiJsText

    # ------------------------------------------------------------------
    # 2a. DOM ID Parity (index.html vs main.js/api.js el() / getElementById())
    # ------------------------------------------------------------------
    $HtmlIds = New-Object System.Collections.Generic.HashSet[string]
    foreach ($m in [regex]::Matches($HtmlText, '\bid\s*=\s*["'']([^"''$]+)["'']')) {
        [void]$HtmlIds.Add($m.Groups[1].Value)
    }
    foreach ($m in [regex]::Matches($CombinedJsText, '\bid\s*=\s*["'']([A-Za-z0-9_-]+)["'']')) {
        [void]$HtmlIds.Add($m.Groups[1].Value)
    }
    $MissingDomIds = New-Object System.Collections.Generic.HashSet[string]
    foreach ($m in [regex]::Matches($CombinedJsText, '(?:\bel|getElementById)\(\s*["'']([A-Za-z0-9_-]+)["'']\s*\)')) {
        $IdName = $m.Groups[1].Value
        if (-not $HtmlIds.Contains($IdName)) {
            [void]$MissingDomIds.Add($IdName)
        }
    }
    Add-CheckResult 'DOM ID Parity' ($MissingDomIds.Count -eq 0) $(if ($MissingDomIds.Count -eq 0) { "$($HtmlIds.Count) IDs indexed, 0 missing" } else { "Missing in index.html: $(($MissingDomIds | Sort-Object) -join ', ')" })

    # ------------------------------------------------------------------
    # 2b. SVG <symbol> Parity (#icon-...)
    # ------------------------------------------------------------------
    $SvgSymbols = New-Object System.Collections.Generic.HashSet[string]
    foreach ($m in [regex]::Matches($HtmlText, '<symbol\b[^>]*\bid\s*=\s*["''](icon-[^"''$]+)["'']')) {
        [void]$SvgSymbols.Add($m.Groups[1].Value)
    }
    $MissingIcons = New-Object System.Collections.Generic.HashSet[string]
    foreach ($m in [regex]::Matches(($HtmlText + "`n" + $CombinedJsText), '#(icon-[A-Za-z0-9_-]+)')) {
        $IconId = $m.Groups[1].Value
        if (-not $SvgSymbols.Contains($IconId)) {
            [void]$MissingIcons.Add($IconId)
        }
    }
    Add-CheckResult 'SVG Sprite Parity' ($MissingIcons.Count -eq 0) $(if ($MissingIcons.Count -eq 0) { "$($SvgSymbols.Count) symbols defined, 0 missing" } else { "Missing <symbol>: $(($MissingIcons | Sort-Object) -join ', ')" })

    # ------------------------------------------------------------------
    # 2c. Tauri IPC Command Parity (invokeCommand / invoke vs generate_handler!)
    # ------------------------------------------------------------------
    $RegisteredCmds = New-Object System.Collections.Generic.HashSet[string]
    $HandlerMatch = [regex]::Match($MainRsText, 'generate_handler!\[\s*([\s\S]*?)\s*\]')
    if ($HandlerMatch.Success) {
        foreach ($RawEntry in ($HandlerMatch.Groups[1].Value -split ',')) {
            $LineNoComment = ($RawEntry -replace '//.*', '').Trim()
            if ($LineNoComment) {
                $CmdLeaf = ($LineNoComment -split '::')[-1].Trim()
                if ($CmdLeaf) { [void]$RegisteredCmds.Add($CmdLeaf) }
            }
        }
    }
    $MissingCmds = New-Object System.Collections.Generic.HashSet[string]
    $InvokedCount = 0
    foreach ($m in [regex]::Matches($CombinedJsText, '(?:\binvokeCommand|\.invoke|\binvoke)\(\s*["'']([a-zA-Z0-9_]+)["'']')) {
        $InvokedCount++
        $CmdName = $m.Groups[1].Value
        if (-not $RegisteredCmds.Contains($CmdName)) {
            [void]$MissingCmds.Add($CmdName)
        }
    }
    $CmdOk = ($RegisteredCmds.Count -gt 0) -and ($MissingCmds.Count -eq 0)
    Add-CheckResult 'Tauri IPC Parity' $CmdOk $(if ($CmdOk) { "$($RegisteredCmds.Count) handlers registered, $InvokedCount frontend calls matched" } else { "Unregistered invokeCommand(s): $(($MissingCmds | Sort-Object) -join ', ')" })

    # ------------------------------------------------------------------
    # 2d. capabilities/default.json Permissions Parity
    # ------------------------------------------------------------------
    $CapJson = [System.IO.File]::ReadAllText($CapPath) | ConvertFrom-Json
    $CapPerms = New-Object System.Collections.Generic.HashSet[string]
    foreach ($Perm in $CapJson.permissions) {
        if ($Perm -is [string]) {
            [void]$CapPerms.Add($Perm)
        } elseif ($Perm.PSObject.Properties['identifier']) {
            [void]$CapPerms.Add([string]$Perm.identifier)
        }
    }
    $RequiredPerms = New-Object System.Collections.Generic.List[string]
    $RequiredPerms.Add('core:default')
    if ($CombinedJsText -match '__TAURI__\??\.window') {
        if ($CombinedJsText -match '\.minimize\(')       { $RequiredPerms.Add('core:window:allow-minimize') }
        if ($CombinedJsText -match '\.toggleMaximize\(') { $RequiredPerms.Add('core:window:allow-toggle-maximize') }
        if ($CombinedJsText -match '\.startDragging\(')  { $RequiredPerms.Add('core:window:allow-start-dragging') }
        if ($CombinedJsText -match '\.close\(')          { $RequiredPerms.Add('core:window:allow-close') }
    }
    if ($CombinedJsText -match '__TAURI__\??\.dialog\??\.open') { $RequiredPerms.Add('dialog:allow-open') }
    if ($CombinedJsText -match '__TAURI__\??\.dialog\??\.save') { $RequiredPerms.Add('dialog:allow-save') }
    if ($CombinedJsText -match '__TAURI__\??\.shell\??\.open')  { $RequiredPerms.Add('shell:allow-open') }
    $CargoText = [System.IO.File]::ReadAllText($CargoTomlPath)
    foreach ($PluginMatch in [regex]::Matches($CargoText, 'tauri-plugin-(dialog|fs|shell)\b')) {
        $RequiredPerms.Add("$($PluginMatch.Groups[1].Value):default")
    }
    $MissingPerms = $RequiredPerms | Select-Object -Unique | Where-Object { -not $CapPerms.Contains($_) }
    $HasMainWindowCap = ($CapJson.windows -contains 'main')
    $CapOk = $HasMainWindowCap -and (@($MissingPerms).Count -eq 0)
    $CapDetail = if ($CapOk) {
        "$($CapPerms.Count) permissions verified for windows [$($CapJson.windows -join ', ')]"
    } else {
        "HasMain=$HasMainWindowCap; Missing: $($MissingPerms -join ', ')"
    }
    Add-CheckResult 'Capabilities Parity' $CapOk $CapDetail

    # ------------------------------------------------------------------
    # 3a. tauri.conf.json Window Transparency & WebView2 Safety
    # ------------------------------------------------------------------
    $TauriRaw = [System.IO.File]::ReadAllText($TauriConfPath)
    $TauriJson = $TauriRaw | ConvertFrom-Json
    $MainWin = $TauriJson.app.windows | Where-Object { $_.label -eq 'main' } | Select-Object -First 1
    $TransOk = ($null -ne $MainWin) -and ($MainWin.transparent -eq $false)
    $SingleProcLeak = $TauriRaw -match '--renderer-process-limit=1|--single-process'
    $TauriOk = $TransOk -and (-not $SingleProcLeak)
    Add-CheckResult 'Tauri Window Config' $TauriOk $(if ($TauriOk) { 'main.transparent = false, multi-process WebView2 intact' } else { "transparent=$($MainWin.transparent), singleProcessLeak=$SingleProcLeak" })

    $CssPath = Join-Path $RepoRoot 'duke-launcher\src\styles.css'
    $CssRaw = [System.IO.File]::ReadAllText($CssPath)
    $SolidThemes = @('dark', 'light', 'minecraft', 'sunset', 'aerospace', 'cyberpunk', 'terminal', 'crimson', 'ocean', 'frost')
    $MissingThemes = @($SolidThemes | Where-Object { $CssRaw -notmatch ("\[data-theme=""$($_)""\]") })
    $ThemesOk = ($MissingThemes.Count -eq 0)
    Add-CheckResult '10 Solid CSS Themes' $ThemesOk $(if ($ThemesOk) { 'All 10 solid themes verified in styles.css' } else { "Missing themes: $($MissingThemes -join ', ')" })

    # ------------------------------------------------------------------
    # 3b. Zero Forbidden Theme & Zero Personal Username/Path Leaks
    # ------------------------------------------------------------------
    $BannedTheme = 'frosted' + '-obsidian'
    $ThemeHits = New-Object System.Collections.Generic.List[string]
    $LeakHits = New-Object System.Collections.Generic.List[string]
    $ActiveUser = $env:USERNAME
    $InGitRepo = Test-Path -LiteralPath (Join-Path $RepoRoot '.git')

    function Test-PersonalLeakLine {
        param(
            [string]$FileRel,
            [string]$LineNo,
            [string]$Content,
            [string]$ActiveUser,
            [System.Collections.Generic.List[string]]$OutLeaks
        )
        foreach ($m in [regex]::Matches($Content, '(?i)[A-Z]:[/\\]+Users[/\\]+([^\s"'';,)\]\\/`]+)')) {
            $Seg = $m.Groups[1].Value
            if ($Seg -notmatch '^(?:<[^>\s]+>|\$\{[^}]+\}|%[^%]+%|Public|Default|All|Demo|Player|User|Example)$') {
                $OutLeaks.Add("${FileRel}:${LineNo} (WinUserPath: $Seg)")
            }
        }
        foreach ($m in [regex]::Matches($Content, '(?<![A-Za-z0-9_])/(?:Users|home)/([A-Za-z0-9._<>-]+)')) {
            $Seg = $m.Groups[1].Value
            if ($Seg -notmatch '^(?:me|deck|steamuser|00000001|<[^>\s]+>|user|example|runner)$') {
                $OutLeaks.Add("${FileRel}:${LineNo} (UnixHomePath: $Seg)")
            }
        }
        if ($ActiveUser -and $ActiveUser.Length -ge 3 -and $Content -match "(?i)(?<![A-Za-z0-9_])$([regex]::Escape($ActiveUser))(?![A-Za-z0-9_])") {
            $OutLeaks.Add("${FileRel}:${LineNo} (ActiveOSUsername)")
        }
    }

    if ($InGitRepo) {
        $PrevPref = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $RawThemeMatches = @(& git grep -I -n -i $BannedTheme 2>$null)
            foreach ($Line in $RawThemeMatches) {
                $Parts = $Line -split ':', 3
                if ($Parts.Count -lt 3) { continue }
                $FileRel = $Parts[0]
                $LineNo = $Parts[1]
                $Content = $Parts[2]
                if ($FileRel -eq 'scripts/verify-hygiene.ps1') { continue }
                if ($FileRel -match '^(?:\.agents/|AGENTS\.md$|CLAUDE\.md$|GEMINI\.md$)' -and $Content -match '(?i)\b(?:zero|never|ban|no|forbid|exclude)\b') {
                    continue
                }
                $ThemeHits.Add("${FileRel}:${LineNo}")
            }

            $WinPathMatches = @(& git grep -I -n -E -i '[A-Z]:[/\\]+Users[/\\]+' 2>$null)
            foreach ($Line in $WinPathMatches) {
                $Parts = $Line -split ':', 3
                if ($Parts.Count -lt 3) { continue }
                if ($Parts[0] -eq 'scripts/verify-hygiene.ps1') { continue }
                Test-PersonalLeakLine -FileRel $Parts[0] -LineNo $Parts[1] -Content $Parts[2] -ActiveUser '' -OutLeaks $LeakHits
            }

            $UnixPathMatches = @(& git grep -I -n -E '(^|[^A-Za-z0-9_])/(Users|home)/[A-Za-z0-9._<>-]+' 2>$null)
            foreach ($Line in $UnixPathMatches) {
                $Parts = $Line -split ':', 3
                if ($Parts.Count -lt 3) { continue }
                if ($Parts[0] -eq 'scripts/verify-hygiene.ps1') { continue }
                Test-PersonalLeakLine -FileRel $Parts[0] -LineNo $Parts[1] -Content $Parts[2] -ActiveUser '' -OutLeaks $LeakHits
            }

            if ($ActiveUser -and $ActiveUser.Length -ge 3) {
                $UserHits = @(& git grep -I -n -w -i $ActiveUser 2>$null)
                foreach ($Line in $UserHits) {
                    $Parts = $Line -split ':', 3
                    if ($Parts.Count -ge 2) {
                        $LeakHits.Add("$($Parts[0]):$($Parts[1]) (ActiveOSUsername)")
                    }
                }
            }

            $Untracked = @(& git ls-files --others --exclude-standard 2>$null)
            foreach ($Rel in $Untracked) {
                if ($Rel -eq 'scripts/verify-hygiene.ps1') { continue }
                if ($Rel -match '\.(png|ico|icns|jpg|jpeg|webp|exe|dll|woff2?|ttf|bin|dat)$') { continue }
                $Abs = Join-Path $RepoRoot $Rel
                if (-not [System.IO.File]::Exists($Abs)) { continue }
                $Txt = [System.IO.File]::ReadAllText($Abs)
                if ($Txt -match $BannedTheme) {
                    if (-not ($Rel -match '^(?:\.agents/|AGENTS\.md$)' -and $Txt -match '(?i)\b(?:zero|never|ban|no)\b')) {
                        $ThemeHits.Add("$Rel (untracked)")
                    }
                }
                Test-PersonalLeakLine -FileRel $Rel -LineNo 'untracked' -Content $Txt -ActiveUser $ActiveUser -OutLeaks $LeakHits
            }
        }
        finally {
            $ErrorActionPreference = $PrevPref
        }
    } else {
        $Stack = New-Object System.Collections.Generic.Stack[string]
        $Stack.Push($RepoRoot)
        while ($Stack.Count -gt 0) {
            $Dir = $Stack.Pop()
            foreach ($SubDir in [System.IO.Directory]::GetDirectories($Dir)) {
                $Leaf = [System.IO.Path]::GetFileName($SubDir)
                if ($Leaf -in @('.git', 'target', 'node_modules', '.design-tools')) { continue }
                $Stack.Push($SubDir)
            }
            foreach ($FilePath in [System.IO.Directory]::GetFiles($Dir)) {
                $Rel = $FilePath.Substring($RepoRoot.Length).TrimStart('\', '/').Replace('\', '/')
                if ($Rel -eq 'scripts/verify-hygiene.ps1') { continue }
                if ($Rel -match '\.(png|ico|icns|jpg|jpeg|webp|exe|dll|woff2?|ttf|bin|dat|pdb|lib)$') { continue }
                $Txt = [System.IO.File]::ReadAllText($FilePath)
                if ($Txt -match $BannedTheme) {
                    if (-not ($Rel -match '^(?:\.agents/|AGENTS\.md$)' -and $Txt -match '(?i)\b(?:zero|never|ban|no|forbid|exclude)\b')) {
                        $ThemeHits.Add($Rel)
                    }
                }
                Test-PersonalLeakLine -FileRel $Rel -LineNo 'scan' -Content $Txt -ActiveUser $ActiveUser -OutLeaks $LeakHits
            }
        }
    }

    Add-CheckResult 'Zero Forbidden Theme' ($ThemeHits.Count -eq 0) $(if ($ThemeHits.Count -eq 0) { '0 banned theme references' } else { $ThemeHits -join '; ' })
    Add-CheckResult 'Zero Personal Path/User Leaks' ($LeakHits.Count -eq 0) $(if ($LeakHits.Count -eq 0) { '0 personal username or home path leaks across tracked + untracked files' } else { $LeakHits -join '; ' })
}
finally {
    Pop-Location
}

$Sw.Stop()
$ElapsedMs = $Sw.ElapsedMilliseconds

$CheckResults | Format-Table -Wrap -AutoSize
Write-Host ("Completed in {0} ms" -f $ElapsedMs)

if ($Failures.Count -gt 0) {
    Write-Error ("Hygiene verification failed ({0} check(s)):`n - {1}" -f $Failures.Count, ($Failures -join "`n - "))
    exit 1
}

exit 0
