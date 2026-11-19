# capri-syn's installer for Windows. One script: Synaps if it's missing, her binary, her art pack, the
# Synaps daemon, the summon key, autostart, and her voice when asked. Every step is idempotent: run it
# again to upgrade. Linux: install.sh, the same steps and flags.
#
#   irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/main/install.ps1))) --yes
#   .\install.ps1 --dry-run          # from a checkout
#
# Flags are install.sh's (--yes, --dry-run, ...) or PowerShell's spelling of them (-Yes, -DryRun, ...).
# The steps, the flags and what each one touches: docs/INSTALL.md. Pins and URLs: installer/manifest.
# Nothing asks for admin, except installing Visual Studio's C++ build tools for a source build, which
# is printed and runs only after you say yes (or --yes).
#
# Windows PowerShell 5.1 and PowerShell 7. ASCII only: 5.1 reads a script without a BOM as ANSI.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'An installer talks to the person running it, in colour.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = '--dry-run is the what-if, as in install.sh.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Synaps is a name; the step functions read as install.sh''s steps.')]
param()

# All of it runs in a module of its own: `irm | iex` runs a script in the caller's scope, and this
# leaves nothing there (no variables, functions or preferences). A cut-off download doesn't parse
# (the module's last line closes it), so it never runs half. Not indented, for diffs.
New-Module -Name capri-install -ReturnResult -ErrorAction Stop -ArgumentList (, [string[]]$args) -ScriptBlock {
param([string[]]$CliArgs)
Export-ModuleMember -Function @()

$RepoDefault = 'HaseebKhalid1507/capri-syn'
$Steps = 7
$Bar = [string][char]0x258C
$Tick = [string][char]0x2713
$Cross = [string][char]0x2717

$Usage = @'
usage: install.ps1 [--yes] [--dry-run] [--uninstall [--purge]] [--voice] [--watch] [--no-autostart]
                   [--from-source] [--prefix DIR] [--with-local-art] [--artifact FILE] [--allow-root]

  --yes             don't ask: Synaps' installer, and Visual Studio's C++ build tools (admin) for a
                    source build
  --dry-run         print every action, touch nothing
  --uninstall       remove what this script installed; your settings stay, and her state and log
                    (her installer's own uninstaller takes those when it runs)
  --purge           with --uninstall: remove those too (only her own files in those folders)
  --voice           also her voice: the voice installer (or bundle), ~320 MB of models
  --watch           capri-watch (Linux only: skipped here)
  --no-autostart    don't start her at sign-in (she still starts now, at the end)
  --from-source     build her from source even when a release has an installer
  --prefix DIR      where she goes (default %LOCALAPPDATA%\Programs\synaps-avatar)
  --with-local-art  also mochi's art: LOCAL PERSONAL USE ONLY, never publish or share it
  --artifact FILE   install this release asset (her setup .exe) instead of downloading one
  --allow-root      install.sh's (installing as root); nothing to allow here

Each flag also has PowerShell's spelling: -Yes, -DryRun, -Uninstall, -Purge, -Voice, -Watch,
-NoAutostart, -FromSource, -Prefix DIR, -WithLocalArt, -Artifact FILE, -AllowRoot.
With no flags (irm | iex can't pass any), $env:CAPRI_INSTALL_FLAGS is read instead (space-separated),
said in the banner, then cleared from your session: it applies to one run only.
Steps and details: docs/INSTALL.md (https://github.com/HaseebKhalid1507/capri-syn)
'@

# ---- ui ---------------------------------------------------------------------------------------

function Write-Info([string]$Text) { Write-Host "   $Text" -ForegroundColor DarkGray }
function Write-Good([string]$Text) {
    Write-Host '   ' -NoNewline
    Write-Host $Tick -ForegroundColor Red -NoNewline
    Write-Host " $Text"
}
function Write-Warn([string]$Text) { Write-Host "   ! $Text" -ForegroundColor Red }
function Write-Step([string]$Name) {
    $script:StepN++
    Write-Host ''
    Write-Host " $Bar" -ForegroundColor Red -NoNewline
    Write-Host " $($script:StepN)/$($script:StepCount) " -ForegroundColor DarkGray -NoNewline
    Write-Host $Name -ForegroundColor White
}
function Stop-Install([string]$Text) {
    Write-Host ''
    Write-Host " $Cross $Text" -ForegroundColor Red
    throw "install.ps1: $Text"
}
function Format-Command([string[]]$Words) {
    ($Words | ForEach-Object { if ($_ -eq '' -or $_ -match '[\s"''&;|<>]') { '"' + $_ + '"' } else { $_ } }) -join ' '
}
function Write-Action([string]$Text) { Write-Host "   `$ $Text" -ForegroundColor DarkGray }
# A native program: printed, run unless --dry-run, its exit code checked.
function Invoke-Native([string]$Exe, [string[]]$ArgList) {
    Write-Action (Format-Command (@($Exe) + $ArgList))
    if ($script:Dry) { return }
    # A native program's stderr (progress, warnings) is not an error: 5.1 would throw on it. Its
    # stdout goes to the console as it comes: in the module it would be held until the end.
    $ErrorActionPreference = 'Continue'
    & $Exe @ArgList | Out-Host
    if ($LASTEXITCODE) { Stop-Install "$(Split-Path -Leaf $Exe) failed (exit $LASTEXITCODE)" }
}
# Anything else: its description printed, the block run unless --dry-run.
function Invoke-Action([string]$Show, [scriptblock]$Do) {
    Write-Action $Show
    if (-not $script:Dry) { & $Do }
}
# Yes with --yes; else asked; no console to ask on: no.
function Confirm-Action([string]$Question) {
    if ($script:Yes) { return $true }
    if ($script:Dry) { Write-Info "(asks first: $Question)"; return $true }
    if (-not [Environment]::UserInteractive -or [Console]::IsInputRedirected) {
        Write-Info "no console to ask on: skipped ($Question; --yes does it)"
        return $false
    }
    Write-Host "   ? $Question [y/N] " -ForegroundColor Red -NoNewline
    $a = Read-Host
    return ($a -match '^(y|yes)$')
}
function Get-Tilde([string]$Path) { if ($Path) { $Path.Replace($env:USERPROFILE, '~') } else { $Path } }

# ---- small tools ------------------------------------------------------------------------------

function Get-Headers {
    $h = @{ 'User-Agent' = 'capri-syn-install' }
    if ($env:GITHUB_TOKEN) { $h['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    $h
}
function Get-Text([string]$Url, [hashtable]$Headers = @{}) {
    $r = Invoke-WebRequest -UseBasicParsing -Uri $Url -Headers $Headers
    if ($r.Content -is [byte[]]) { [Text.Encoding]::UTF8.GetString($r.Content) } else { [string]$r.Content }
}
function Get-Download([string]$Url, [string]$Out) {
    Write-Action "Invoke-WebRequest $Url -OutFile $Out"
    if ($script:Dry) { return }
    New-Item -ItemType Directory -Force (Split-Path -Parent $Out) | Out-Null
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Out
}
function Get-Sha256([string]$Path) { (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant() }
function Test-Sha256([string]$Path, [string]$Want) {
    if ($script:Dry) { Write-Info "(checks its sha256: $Want)"; return }
    $got = Get-Sha256 $Path
    if ($got -ne $Want) { Stop-Install "$(Split-Path -Leaf $Path): sha256 $got is not the expected $Want" }
    Write-Good "sha256 checked: $(Split-Path -Leaf $Path)"
}

# The receipt: what this script installed, one entry per line, tab-separated, the path last: prefix,
# file, dir, rmdir (removed only if empty), inno KEY, reg KEY NAME, regkey KEY, meta KEY VALUE.
# --uninstall reads it backwards, and removes only paths in her folders (Test-Removable).
function Add-Receipt([string[]]$Fields) {
    if ($script:Dry) { return }
    $line = $Fields -join "`t"
    New-Item -ItemType Directory -Force (Split-Path -Parent $script:Receipt) | Out-Null
    if (-not (Test-Path $script:Receipt) -or -not (@(Get-Content -LiteralPath $script:Receipt) -contains $line)) {
        Add-Content -LiteralPath $script:Receipt -Value $line -Encoding UTF8
    }
}
function Set-ReceiptMeta([string]$Key, [string]$Value) {
    if ($script:Dry) { return }
    $keep = @()
    if (Test-Path $script:Receipt) { $keep = @(Get-Content -LiteralPath $script:Receipt | Where-Object { $_ -notlike "meta`t$Key`t*" }) }
    New-Item -ItemType Directory -Force (Split-Path -Parent $script:Receipt) | Out-Null
    Set-Content -LiteralPath $script:Receipt -Value (@($keep) + "meta`t$Key`t$Value") -Encoding UTF8
}
function Get-ReceiptMeta([string]$Key) {
    if (-not (Test-Path $script:Receipt)) { return $null }
    $l = Get-Content -LiteralPath $script:Receipt | Where-Object { $_ -like "meta`t$Key`t*" } | Select-Object -Last 1
    if ($l) { return ($l -split "`t", 3)[2] } else { return $null }
}
function Test-Recorded([string]$Kind, [string]$Path) {
    (Test-Path $script:Receipt) -and [bool](Get-Content -LiteralPath $script:Receipt | Where-Object { $_ -eq "$Kind`t$Path" })
}
# A folder that is all hers: --uninstall removes it whole (only if this made it).
function New-RecordedDir([string]$Path) {
    if (-not (Test-Path $Path)) {
        Invoke-Action "mkdir $Path" { New-Item -ItemType Directory -Force $Path | Out-Null }
        Add-Receipt @('dir', $Path)
    }
}
# mkdir, each folder it makes recorded as rmdir: --uninstall removes those only if they're empty.
function New-RecordedPath([string]$Path) {
    $todo = @()
    $d = $Path
    while ($d -and -not (Test-Path -LiteralPath $d) -and $d -ne $env:USERPROFILE) {
        $todo = @($d) + $todo
        $d = Split-Path -Parent $d
    }
    if (-not $todo) { return }
    Invoke-Action "mkdir $Path" { New-Item -ItemType Directory -Force $Path | Out-Null }
    foreach ($t in $todo) { Add-Receipt @('rmdir', $t) }
}
# May --uninstall remove PATH? Rooted, one line, no `..`, never a drive's root, his profile, LocalAppData
# or AppData; inside one of her folders (each prefix, her data and settings, capri-voice's models
# folder), or her Start-menu shortcut. A dir (removed whole) must be one of the folders this makes. An
# rmdir (only ever an empty folder) may also be one of her folders, or one on the way to one.
function Test-Removable([string]$Path, [string]$Kind) {
    if (-not $Path -or $Path -match "[`r`n]" -or -not [IO.Path]::IsPathRooted($Path) -or $Path -match '(^|[\\/])\.\.?([\\/]|$)') { return $false }
    $p = $Path.TrimEnd('\', '/')
    $never = @($env:USERPROFILE, $env:LOCALAPPDATA, $env:APPDATA, [IO.Path]::GetPathRoot($Path)) | ForEach-Object { "$_".TrimEnd('\', '/') }
    if (-not $p -or $never -contains $p) { return $false }
    if ($p -eq $script:Lnk) { return $true }
    $voice = Join-Path $env:LOCALAPPDATA 'capri-voice'
    if ($Kind -eq 'dir') {
        # Removed whole: only the folders this script makes (her pack, its cache, a voice bundle, the models).
        $known = @((Join-Path $script:Data 'packs\capri'), (Join-Path $script:Data 'installer'), $voice) + @($script:Prefixes | Where-Object { $_ } | ForEach-Object { Join-Path "$_".TrimEnd('\', '/') 'voice' })
        return [bool]($known | Where-Object { [string]::Equals("$_".TrimEnd('\', '/'), $p, [StringComparison]::OrdinalIgnoreCase) })
    }
    foreach ($r in (@($script:Prefixes) + @($script:Data, $script:Conf, $voice))) {
        $r = "$r".TrimEnd('\', '/')
        if (-not $r) { continue }
        if ($p.StartsWith("$r\", [StringComparison]::OrdinalIgnoreCase)) { return $true }
        if ($Kind -eq 'rmdir' -and "$r\".StartsWith("$p\", [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
    return $false
}
# A Run value: set and recorded, unless one of that name is there that this script didn't set (his:
# left as it is, never recorded, so --uninstall leaves it too).
function Set-RunValue([string]$Name, [string]$Value) {
    $run = $script:RunKey
    $cur = (Get-ItemProperty -Path $run -Name $Name -ErrorAction SilentlyContinue).$Name
    if ($cur -eq $Value) { Write-Good "already set: Run\$Name"; return }
    if ($null -ne $cur -and -not (Test-Recorded 'reg' "$run`t$Name")) { Write-Good "your own Run\$Name ($cur): left as it is"; return }
    Invoke-Action "reg add HKCU\...\Run /v $Name /d $Value" { New-ItemProperty -Path $run -Name $Name -Value $Value -PropertyType String -Force | Out-Null }
    Add-Receipt @('reg', $run, $Name)
}

# ---- setup ------------------------------------------------------------------------------------

function Read-Flags([string[]]$ArgList) {
    $script:Yes = $false; $script:Dry = $false; $script:Uninstall = $false; $script:Purge = $false
    $script:Voice = $false; $script:Watch = $false; $script:Autostart = $true; $script:FromSource = $false
    $script:LocalArt = $false; $script:Prefix = Join-Path $env:LOCALAPPDATA 'Programs\synaps-avatar'; $script:Artifact = ''; $script:PrefixGiven = $false
    $script:HisRun = ''; $script:StoppedElsewhere = $false
    $i = 0
    while ($ArgList -and $i -lt $ArgList.Count) {
        $a = [string]$ArgList[$i]
        $val = $null
        if ($a -match '^(-{1,2}[A-Za-z-]+)[=:](.*)$') { $a = $Matches[1]; $val = $Matches[2] }
        $k = $a.TrimStart('-').Replace('-', '').ToLowerInvariant()
        switch ($k) {
            { $_ -in 'y', 'yes' } { $script:Yes = $true }
            { $_ -in 'n', 'dryrun' } { $script:Dry = $true }
            'uninstall' { $script:Uninstall = $true }
            'purge' { $script:Purge = $true }
            'voice' { $script:Voice = $true }
            'watch' { $script:Watch = $true }
            'noautostart' { $script:Autostart = $false }
            'fromsource' { $script:FromSource = $true }
            'withlocalart' { $script:LocalArt = $true }
            'allowroot' { }
            { $_ -in 'prefix', 'artifact' } {
                if ($null -eq $val) {
                    $i++
                    if ($i -ge $ArgList.Count) { Stop-Install "--$k needs a value" }
                    $val = [string]$ArgList[$i]
                }
                if ($k -eq 'prefix') { $script:Prefix = $val; $script:PrefixGiven = $true } else { $script:Artifact = $val }
            }
            { $_ -in 'h', 'help' } { Write-Output $Usage; $script:HelpOnly = $true; return }
            default { Write-Host $Usage; Stop-Install "unknown flag: $a" }
        }
        $i++
    }
    if ($script:Purge -and -not $script:Uninstall) { Stop-Install '--purge goes with --uninstall' }
    if ($script:Prefix -match "[`r`n]") { Stop-Install '--prefix: a folder name with a newline in it' }
    # Relative to PowerShell's location (not the process's), `~` expanded; then `..` resolved.
    $script:Prefix = [IO.Path]::GetFullPath($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($script:Prefix)).TrimEnd('\', '/')
    if ($script:Prefix -eq [IO.Path]::GetPathRoot($script:Prefix).TrimEnd('\', '/')) { Stop-Install "--prefix $($script:Prefix): give her a folder of her own" }
    if ($script:Artifact) {
        if (-not (Test-Path -LiteralPath $script:Artifact)) { Stop-Install "--artifact: no file $($script:Artifact)" }
        $script:Artifact = (Resolve-Path -LiteralPath $script:Artifact).Path
    }
}

function Read-Manifest {
    $script:Ref = if ($env:CAPRI_INSTALL_REF) { $env:CAPRI_INSTALL_REF } else { 'main' }
    $script:Checkout = ''
    if ($PSScriptRoot -and (Test-Path (Join-Path $PSScriptRoot 'installer\manifest')) -and (Test-Path (Join-Path $PSScriptRoot 'Cargo.toml'))) {
        $script:Checkout = $PSScriptRoot
        $text = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'installer\manifest')
    } else {
        try { $text = Get-Text "https://raw.githubusercontent.com/$RepoDefault/$($script:Ref)/installer/manifest" }
        catch { Stop-Install "can't fetch the installer's manifest from GitHub ($($script:Ref)): $($_.Exception.Message)" }
    }
    $m = @{}
    foreach ($line in ($text -split "`r?`n")) {
        $l = $line.Trim()
        if (-not $l -or $l.StartsWith('#')) { continue }
        $eq = $l.IndexOf('=')
        if ($eq -lt 1) { Stop-Install "manifest: bad line '$l'" }
        $m[$l.Substring(0, $eq)] = $l.Substring($eq + 1)
    }
    foreach ($k in 'repo', 'synaps_repo', 'synaps_min', 'asset_windows', 'asset_windows_voice', 'rustup_dist') {
        if (-not $m[$k]) { Stop-Install "manifest: no $k" }
    }
    $script:M = $m
    $script:ManifestText = $text
}

function Set-Paths {
    $script:Data = Join-Path $env:LOCALAPPDATA 'synaps-avatar'
    $script:Conf = Join-Path $env:APPDATA 'synaps-avatar'
    $script:Pack = Join-Path $script:Data 'packs\capri'
    $script:Cache = Join-Path $script:Data 'installer'
    $script:Receipt = Join-Path $script:Data 'install.receipt'
    $script:Exe = Join-Path $script:Prefix 'synaps-avatar.exe'
    $script:Lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\synaps-avatar.lnk'
    $script:RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    $script:Aumid = 'HKCU:\Software\Classes\AppUserModelId\Synaps.Avatar'
    $script:CargoBin = if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' } else { Join-Path $env:USERPROFILE '.cargo\bin' }
    $script:InnoKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{2147942F-E90C-4536-BEA2-8AB4B7F51734}_is1'
    $script:VoiceKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7E0CA8F-064C-4B7F-939A-41681146411C}_is1'
}

function Write-Banner {
    $what = if ($script:Uninstall) { 'uninstall' } else { 'install' }
    Write-Host ''
    Write-Host " $($Bar)capri" -ForegroundColor Red -NoNewline
    Write-Host " > $what" -ForegroundColor DarkGray -NoNewline
    if ($script:Dry) { Write-Host '  (dry run: nothing is touched)' -ForegroundColor Red } else { Write-Host '' }
    if ($script:Checkout) { Write-Info "from the checkout $(Get-Tilde $script:Checkout)" }
    if ($script:FlagsEnv -like 'used:*') { Write-Info "flags from CAPRI_INSTALL_FLAGS: $($script:FlagsEnv.Substring(5)) (cleared: it applies to this run only)" }
    elseif ($script:FlagsEnv -like 'ignored:*') { Write-Info "CAPRI_INSTALL_FLAGS ($($script:FlagsEnv.Substring(8))) ignored: flags were given (cleared)" }
}

# Her source tree: this checkout, else GitHub's tarball of a ref (her make-pack and its pins, and
# the code for a source build).
function Get-Source([string]$Ref) {
    if ($script:Checkout) { return $script:Checkout }
    if ($script:SrcDir) { return $script:SrcDir }
    $sha = $null
    try { $sha = (Get-Text "https://api.github.com/repos/$($script:M.repo)/commits/$Ref" ((Get-Headers) + @{ Accept = 'application/vnd.github.sha' })).Trim() } catch { $sha = $null }
    if ($sha -notmatch '^[0-9a-f]{40}$') { $sha = if ($Ref -match '^[0-9a-f]{40}$') { $Ref } else { $null } }
    if ($sha) {
        $short = $sha.Substring(0, 12)
        $dir = Join-Path $script:Cache "src\capri-syn-$short"
        Write-Info "her files: $($script:M.repo) @ $short ($Ref)"
    } else {
        # Not a commit (GitHub's API didn't answer): this run's own copy, never reused.
        $sha = $Ref; $short = $Ref
        $dir = Join-Path $script:Tmp 'src'
        Write-Info "her files: $($script:M.repo) @ $Ref (which commit, GitHub's API didn't say: not cached)"
    }
    if (-not (Test-Path (Join-Path $dir 'Cargo.toml'))) {
        New-RecordedDir $script:Cache
        $tgz = Join-Path $script:Tmp 'src.tar.gz'
        Get-Download "https://codeload.github.com/$($script:M.repo)/tar.gz/$sha" $tgz
        Invoke-Action "mkdir $dir" { New-Item -ItemType Directory -Force $dir | Out-Null }
        Invoke-Native 'tar.exe' @('-xzf', $tgz, '-C', $dir, '--strip-components=1')
    }
    $script:SrcDir = $dir
    $script:SrcSha = $short
    return $dir
}

# The install steps both installers run (packaging/windows/steps.ps1: her installer runs the same
# file): this checkout's, else fetched from GitHub at this script's ref, beside the manifest this run
# read (the tree it looks for: steps.ps1 and installer\manifest).
function Get-StepScript {
    if ($script:Checkout) { return (Join-Path $script:Checkout 'packaging\windows\steps.ps1') }
    if ($script:StepScript) { return $script:StepScript }
    $dir = Join-Path $script:Tmp 'steps'
    New-Item -ItemType Directory -Force (Join-Path $dir 'installer') | Out-Null
    Set-Content -LiteralPath (Join-Path $dir 'installer\manifest') -Value $script:ManifestText -Encoding Ascii
    $f = Join-Path $dir 'steps.ps1'
    try { Invoke-WebRequest -UseBasicParsing -Uri "https://raw.githubusercontent.com/$RepoDefault/$($script:Ref)/packaging/windows/steps.ps1" -OutFile $f }
    catch { Stop-Install "can't fetch her install steps from GitHub ($($script:Ref)): $($_.Exception.Message)" }
    $script:StepScript = $f
    return $f
}

# One shared step, in a PowerShell of its own: its lines shown as they come (as this script's own),
# its results (`key: value` at the left edge) returned. A failure stops the install with its reason
# (in plain words), unless -NoStop.
function Invoke-Step([string]$Script, [string[]]$ArgList, [switch]$NoStop) {
    $r = @{}
    $ps = (Get-Process -Id $PID).Path
    $enc = [Console]::OutputEncoding
    $ErrorActionPreference = 'Continue'
    try {
        # The step writes UTF-8 (a path can have any letter in it).
        try { [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch { $null = $_ }
        & $ps -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $Script @ArgList 2>&1 | ForEach-Object {
            $l = "$_"
            if ($l -match '^([a-z]+): (.*)$') { $r[$Matches[1]] = $Matches[2] }
            elseif ($l -match '^   \+ (.*)$') { Write-Good $Matches[1] }
            elseif ($l -match '^   ! (.*)$') { Write-Warn $Matches[1] }
            elseif ($l -match '^   \$ (.*)$') { Write-Action $Matches[1] }
            elseif ($l.Trim()) { Write-Info $l.Trim() }
        }
        $r['exit'] = $LASTEXITCODE
    } finally {
        try { [Console]::OutputEncoding = $enc } catch { $null = $_ }
    }
    if ($r['failed']) { Write-Warn $r['failed'] }
    if ($r['exit'] -and -not $NoStop) {
        Stop-Install $(if ($r['failed']) { $r['failed'] } else { "$(Split-Path -Leaf $Script) $($ArgList[0]) failed (exit $($r['exit']))" })
    }
    return $r
}
# A step's `synaps:` result: State (ok, old, missing, would), Version, Path.
function Read-SynapsResult([string]$Line) {
    if ($Line -match '^(\S+) (\S*) (.+)$') { return @{ State = $Matches[1]; Version = $Matches[2]; Path = $Matches[3] } }
    return @{ State = "$Line".Trim(); Version = ''; Path = (Join-Path $script:CargoBin 'synaps.exe') }
}

# ---- 1. synaps --------------------------------------------------------------------------------

function Install-Synaps {
    Write-Step 'Synaps'
    # Found, and new enough (steps.ps1 synaps-check: `synaps` on PATH, else ~\.cargo\bin).
    $steps = Get-StepScript
    $c = Invoke-Step $steps @('synaps-check')
    $s = Read-SynapsResult $c['synaps']
    $script:Brain = ($c['brain'] -eq 'yes')
    if ($s.State -eq 'ok') {
        $script:Synaps = $s.Path; $script:SynapsV = $s.Version
        return
    }
    if ($s.State -eq 'old') {
        Write-Info "installing the latest"
        $script:SynapsNew = 2
    } else {
        Write-Info "her brain runs on Synaps, an open-source agent runtime (github.com/$($script:M.synaps_repo))"
        Write-Info 'not installed yet: installing the latest with its own installer (yours to keep; --uninstall says how to remove it)'
        $script:SynapsNew = 1
    }
    $edit = Get-SynapsPathEdit
    if ($edit) { Write-Info "its installer puts $(Get-Tilde $script:CargoBin) on your user Path (HKCU\Environment, at the front)" }
    else { Write-Info 'its installer leaves your user Path as it is' }
    if (-not (Confirm-Action "install Synaps with its installer$(if ($edit) { ' (and that Path edit)' })")) {
        Write-Info "or install it yourself, your Path untouched: `$env:SYNAPS_NO_MODIFY_PATH = '1'; irm https://github.com/$($script:M.synaps_repo)/releases/latest/download/synaps-installer.ps1 | iex"
        Stop-Install 'she needs Synaps: say yes (or --yes), or install it as above, then run this again'
    }
    # The latest release's installer, sha256 checked (else the manifest's pin), run: steps.ps1 synaps.
    $r = Invoke-Step $steps @(@('synaps') + $(if ($script:Dry) { @('-DryRun') } else { @() }))
    $s = Read-SynapsResult $r['synaps']
    $script:Synaps = $s.Path; $script:SynapsV = $s.Version
    $script:Brain = ($r['brain'] -eq 'yes')
    if ($env:Path -notlike "*$($script:CargoBin)*") { $env:Path = "$($script:CargoBin);$env:Path" }
    # Hers to mention removing only if there was none: an upgrade of his stays his.
    if ($script:SynapsNew -eq 1) { Set-ReceiptMeta 'synaps' $script:Synaps; Set-ReceiptMeta 'synaps_path' $(if ($edit) { 'yes' } else { 'no' }) }
}

# Whether Synaps' installer (cargo-dist) will add its bin folder to his user Path: not when it's there
# already, or SYNAPS_NO_MODIFY_PATH / SYNAPS_UNMANAGED_INSTALL / INSTALLER_NO_MODIFY_PATH say so.
function Get-SynapsPathEdit {
    if ($env:SYNAPS_NO_MODIFY_PATH -or $env:SYNAPS_UNMANAGED_INSTALL -or $env:INSTALLER_NO_MODIFY_PATH) { return $false }
    try { $cur = @((Get-Item -LiteralPath 'registry::HKEY_CURRENT_USER\Environment').GetValue('Path', '', 'DoNotExpandEnvironmentNames') -split ';' | Where-Object { $_ }) }
    catch { return $true }
    return -not ($cur -contains $script:CargoBin)
}

# ---- 2. her binary ----------------------------------------------------------------------------

# A command line's arguments after its program (quoted or not): '' when it was started bare.
function Get-CommandArgs([string]$Line) {
    $l = $Line.TrimStart()
    $rest = if ($l.StartsWith('"')) { $i = $l.IndexOf('"', 1); if ($i -lt 0) { '' } else { $l.Substring($i + 1) } }
    else { $i = $l.IndexOfAny([char[]]" `t"); if ($i -lt 0) { '' } else { $l.Substring($i) } }
    return $rest.Trim()
}

# Her runs, his own, in every sign-in session (one in another, his desktop seen from ssh, holds her
# binary too): Id, Session, Here (in this one), and Line, the command line of one started with
# arguments (--socket, --profile, --session...: his own run), '' for one started bare (as the Start
# menu and her Run value start her).
function Get-HerRuns {
    $here = (Get-Process -Id $PID).SessionId
    foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name = 'synaps-avatar.exe'" -ErrorAction SilentlyContinue)) {
        $o = Invoke-CimMethod -InputObject $p -MethodName GetOwner -ErrorAction SilentlyContinue
        if (-not $o -or $o.User -ne [Environment]::UserName) { continue }
        $line = [string]$p.CommandLine
        [pscustomobject]@{ Id = [int]$p.ProcessId; Session = [int]$p.SessionId; Here = ($p.SessionId -eq $here); Line = $(if (Get-CommandArgs $line) { $line.Trim() } else { '' }) }
    }
}

# A run of hers as a stop names it: its pid, its sign-in session when it isn't this one, its
# command line when he started it with his own arguments.
function Get-RunLabel($Run) {
    $s = "pid $($Run.Id)"
    if (-not $Run.Here) { $s += " in your other sign-in session ($($Run.Session))" }
    if ($Run.Line) { $s += ", as you started her: $($Run.Line)" }
    return $s
}

# Every run of his stops, in every sign-in session (Windows locks a running binary: one anywhere
# would keep the old one), each one said with Why. One he started with his own arguments is noted
# ($script:HisRun): she isn't started bare in its place. One in another session is noted too
# ($script:StoppedElsewhere): from here she can't be started there again.
function Stop-Her([string]$Why) {
    if ($script:Dry) { return }
    $runs = @(Get-HerRuns)
    foreach ($r in $runs) {
        if ($r.Line -and -not $script:HisRun) { $script:HisRun = $r.Line }
        if (-not $r.Here) { $script:StoppedElsewhere = $true }
        Invoke-Action "Stop-Process -Id $($r.Id) ($Why; $(Get-RunLabel $r))" { Stop-Process -Id $r.Id -Force -ErrorAction SilentlyContinue }
    }
    if ($runs -and -not $script:Dry) { Start-Sleep -Milliseconds 500 }
}

# Her installer (packaging/windows), unattended; her autostart is step 6's, not its task.
function Install-Setup([string]$Setup) {
    Stop-Her 'her binary is replaced'
    New-RecordedDir $script:Cache
    # Its folder and the ones on the way, made here: recorded (rmdir), so --uninstall takes them away
    # once her installer's uninstaller has emptied them.
    New-RecordedPath $script:Prefix
    $log = Join-Path $script:Cache 'setup.log'
    # Her binary only: Synaps, her pack, the daemon, autostart and voice are this script's own steps.
    $argLine = "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /DIR=`"$($script:Prefix)`" /SYNAPS=skip /PACK=skip /VOICE=off /BY=install.ps1 /MERGETASKS=`"!autostart`" /LOG=`"$log`""
    Write-Action "$(Split-Path -Leaf $Setup) $argLine"
    if (-not $script:Dry) {
        $p = Start-Process -FilePath $Setup -ArgumentList $argLine -Wait -PassThru
        if ($p.ExitCode -ne 0) { Stop-Install "her installer failed (exit $($p.ExitCode)); its log: $log" }
        if (-not (Test-Path $script:Exe)) { Stop-Install "her installer ran, but there's no $($script:Exe)" }
    }
    Add-Receipt @('inno', $script:InnoKey)
}

function Install-Binary {
    Write-Step 'Her binary'
    $script:BinVer = ''
    if ($script:Artifact) {
        Write-Info "from $(Get-Tilde $script:Artifact)"
        Install-Setup $script:Artifact
        # Her installer records its version (Cargo.toml's) in her Apps entry.
        $v = (Get-ItemProperty -Path $script:InnoKey -ErrorAction SilentlyContinue).DisplayVersion
        $script:BinVer = if ($v) { "$v (artifact)" } else { 'artifact' }
        Set-ReceiptMeta 'binary' 'artifact'
        Write-Good "synaps-avatar $($script:BinVer) ($(Get-Tilde $script:Exe))"
        return
    }
    $rel = "https://github.com/$($script:M.repo)/releases/latest/download"
    $sum = $null
    if ($script:FromSource) {
        Write-Info '--from-source: building her'
    } else {
        try { $sum = ((Get-Text "$rel/$($script:M.asset_windows).sha256") -split '\s+')[0] } catch { $sum = $null }
        if ($sum -notmatch '^[0-9a-f]{64}$') { $sum = $null; Write-Info 'no release yet: building her from source' }
    }
    if ($sum) {
        try { $script:ReleaseTag = (Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$($script:M.repo)/releases/latest" -Headers (Get-Headers)).tag_name } catch { $script:ReleaseTag = $null }
        if ((Get-ReceiptMeta 'setup') -eq $sum -and (Test-Path $script:Exe)) {
            Write-Good "up to date: the latest release ($($script:ReleaseTag))"
            $script:BinVer = $script:ReleaseTag
            return
        }
        $setup = Join-Path $script:Tmp $script:M.asset_windows
        Get-Download "$rel/$($script:M.asset_windows)" $setup
        Test-Sha256 $setup $sum
        Install-Setup $setup
        Set-ReceiptMeta 'setup' $sum
        $script:BinVer = $script:ReleaseTag
        Write-Good "synaps-avatar $($script:BinVer), the latest release ($(Get-Tilde $script:Exe))"
        return
    }
    Build-FromSource
}

function Find-VCTools([switch]$AnyVS) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $null }
    # -AnyVS: Build Tools or a Visual Studio edition, with or without the C++ tools (only those four:
    # SSMS and other products on the same installer can't take the C++ workload).
    $req = if ($AnyVS) {
        @('-products', 'Microsoft.VisualStudio.Product.BuildTools', 'Microsoft.VisualStudio.Product.Community',
            'Microsoft.VisualStudio.Product.Professional', 'Microsoft.VisualStudio.Product.Enterprise')
    } else { @('-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64') }
    $p = & $vswhere -latest @req -property installationPath
    if ($p) { return [string]$p } else { return $null }
}

function Build-FromSource {
    $src = Get-Source $script:Ref
    $vs = if (Find-VCTools) { $null } else { Find-VCTools -AnyVS }
    if ($vs) {
        # winget can't add a workload to a Visual Studio that's there (it says "installed" and stops):
        # its own installer does, and that's his call (admin, ~2 GB).
        $vsi = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\setup.exe'
        Write-Info "Visual Studio is here ($vs) without its C++ build tools, which Rust on Windows links with. Add them"
        Write-Info "in the Visual Studio Installer (Modify, then 'Desktop development with C++'), or as admin:"
        Write-Info "  $(Format-Command @($vsi, 'modify', '--installPath', $vs, '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended', '--passive'))"
        if ($script:Dry) { Write-Info '(a real run stops here)' } else { Stop-Install 'then run this again' }
    } elseif (-not (Find-VCTools)) {
        $wg = @('install', '--id', 'Microsoft.VisualStudio.2022.BuildTools', '-e', '--accept-package-agreements', '--accept-source-agreements',
            '--override', '--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
        Write-Info "Rust on Windows links with Visual Studio's C++ build tools (not installed; ~3 GB, admin):"
        Write-Info "  winget $(Format-Command $wg)"
        if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
            if ($script:Dry) { Write-Warn 'no winget: a real run stops here (install the C++ build tools by hand)' }
            else { Stop-Install 'no winget: install the C++ build tools from https://visualstudio.microsoft.com/visual-cpp-build-tools/, then run this again' }
        }
        if (-not (Confirm-Action 'install them with winget (a UAC prompt)')) { Stop-Install 'run that (or this script with --yes), then run this again' }
        Write-Action (Format-Command (@('winget') + $wg))
        if (-not $script:Dry) {
            $ErrorActionPreference = 'Continue'
            & winget @wg | Out-Host
            $rc = $LASTEXITCODE
            # 0x8A15002B: Settings > Apps lists Build Tools with nothing newer to install, yet Visual
            # Studio's installer has no record of it (an interrupted install). winget then installs
            # nothing; --force runs it again, which works once its old folder is gone (VS won't install
            # into a folder that isn't empty, and winget still says it succeeded).
            if ($rc -eq -1978335189) {
                $bt = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\BuildTools'
                if ((Test-Path -LiteralPath $bt) -and (Get-ChildItem -Force -LiteralPath $bt | Select-Object -First 1)) {
                    Stop-Install "Visual Studio Build Tools 2022 is listed in Settings > Apps, but Visual Studio's installer has no record of it (an interrupted install), and its folder isn't empty, so it can't be installed again. As admin, delete $bt, then run this again"
                }
                Write-Info "winget lists it as installed, but it isn't there (an interrupted install): it can install it again"
                if (-not (Confirm-Action 'install them again with winget --force (another UAC prompt)')) {
                    Stop-Install 'run that with --force (or this script with --yes), then run this again'
                }
                Write-Action (Format-Command (@('winget') + $wg + '--force'))
                & winget @($wg + '--force') | Out-Host
                $rc = $LASTEXITCODE
            }
            $ErrorActionPreference = 'Stop'
            if ($rc) { Stop-Install "winget failed (exit $rc)" }
            if (-not (Find-VCTools)) { Stop-Install "winget finished, but Visual Studio's C++ build tools still aren't there (the UAC prompt declined?): run this again, or install 'Desktop development with C++' from https://visualstudio.microsoft.com/visual-cpp-build-tools/" }
            Write-Good "Visual Studio's C++ build tools"
        }
    } else {
        Write-Good "Visual Studio's C++ build tools"
    }
    $rustup = Join-Path $script:CargoBin 'rustup.exe'
    if (-not (Test-Path $rustup)) {
        $c = Get-Command rustup -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($c) { $rustup = $c.Source }
    }
    if (Test-Path $rustup) {
        Write-Good "rustup ($(Get-Tilde $rustup))"
    } else {
        Write-Info 'no rustup: installing it (user-local, --no-modify-path)'
        $init = Join-Path $script:Tmp 'rustup-init.exe'
        $u = "$($script:M.rustup_dist)/x86_64-pc-windows-msvc/rustup-init.exe"
        Get-Download $u $init
        if (-not $script:Dry) { Test-Sha256 $init (((Get-Text "$u.sha256") -split '\s+')[0]) }
        Invoke-Native $init @('-y', '-q', '--no-modify-path', '--profile', 'minimal', '--default-toolchain', 'none')
        $rustup = Join-Path $script:CargoBin 'rustup.exe'
        Set-ReceiptMeta 'rustup' $rustup
    }
    $cargo = Join-Path (Split-Path -Parent $rustup) 'cargo.exe'
    $target = if ($script:Checkout) { Join-Path $src 'target' } else { Join-Path $script:Cache 'target' }
    Write-Info "in $(Get-Tilde $src): the toolchain rust-toolchain.toml pins, then her build (windowed)"
    if (-not $script:Dry) { Push-Location $src }
    try {
        if (-not $script:Dry) {
            $ErrorActionPreference = 'Continue'
            & $rustup show active-toolchain *> $null
            $ErrorActionPreference = 'Stop'
        }
        if ($script:Dry -or $LASTEXITCODE) { Invoke-Native $rustup @('toolchain', 'install') }
        $env:CARGO_TARGET_DIR = $target
        Write-Info 'building (release; the first build takes a while, later ones only what changed)'
        $t0 = [Diagnostics.Stopwatch]::StartNew()   # not the wall clock: a time sync can jump it
        Invoke-Native $cargo @('rustc', '--locked', '--release', '--bin', 'synaps-avatar', '--', '-C', 'link-arg=/SUBSYSTEM:WINDOWS', '-C', 'link-arg=/ENTRY:mainCRTStartup')
        if (-not $script:Dry) { Write-Good "built in $([Math]::Max(1, [int]$t0.Elapsed.TotalMinutes)) min" }
    } finally {
        if (-not $script:Dry) { Pop-Location }
        Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    }
    Stop-Her 'her binary is replaced'
    New-RecordedPath $script:Prefix
    $built = Join-Path $target 'release\synaps-avatar.exe'
    Invoke-Action "copy $built $($script:Exe)" { Copy-Item -Force $built $script:Exe }
    Add-Receipt @('file', $script:Exe)
    # What her installer does besides: her AUMID (toasts under her name) and a Start-menu shortcut.
    $aumid = $script:Aumid
    if (-not (Test-Path $aumid)) {
        Invoke-Action "reg add HKCU\Software\Classes\AppUserModelId\Synaps.Avatar /v DisplayName /d synaps-avatar" {
            New-Item -Force $aumid | Out-Null
            New-ItemProperty -Path $aumid -Name DisplayName -Value 'synaps-avatar' -PropertyType String -Force | Out-Null
        }
        Add-Receipt @('regkey', $aumid)
    }
    $lnk = $script:Lnk
    Invoke-Action "shortcut $lnk -> $($script:Exe)" {
        $sh = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
        $sh.TargetPath = $script:Exe
        $sh.WorkingDirectory = $script:Prefix
        $sh.Description = 'Capri, on your desktop'
        $sh.Save()
    }
    Add-Receipt @('file', $lnk)
    $script:BinVer = if ($script:Checkout) { 'checkout' } else { $script:SrcSha }
    Set-ReceiptMeta 'binary' $script:BinVer
    if (-not $script:Dry) { Write-Good "synaps-avatar $($script:BinVer) ($(Get-Tilde $script:Exe))" }
}

# ---- 3. her art pack --------------------------------------------------------------------------

function Find-Python {
    $ErrorActionPreference = 'Continue'
    foreach ($c in @(@('py', '-3'), @('python'), @('python3'))) {
        $cmd = Get-Command $c[0] -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $cmd -or $cmd.Source -like '*\WindowsApps\*') { continue }
        $pre = @($c | Select-Object -Skip 1)
        $ok = & $cmd.Source @pre -c 'import sys, venv; print(sys.version_info >= (3, 9))' 2>$null
        if ($ok -eq 'True') {
            [string[]]$r = @($cmd.Source) + $pre
            return , $r
        }
    }
    return $null
}

# The pack step both installers run (steps.ps1 pack): Python (CPython's embeddable package and pip,
# pinned, no Python of his needed) and CapriCore in the cache, make-pack with lachy's files, every file
# checked against tools/*.sha256, her --check, then into place (a pack it didn't make moved aside).
# Its tree is her source's: make-pack and the pins match her binary.
function Install-Pack {
    Write-Step 'Her art pack'
    $src = Get-Source $(if ($script:ReleaseTag) { $script:ReleaseTag } else { $script:Ref })
    if ($script:LocalArt) { Write-Warn "--with-local-art: mochi's art is for LOCAL PERSONAL USE ONLY: never commit, publish or share it" }
    $steps = Join-Path $src 'packaging\windows\steps.ps1'
    $a = @('pack', '-Out', $script:Pack, '-Cache', $script:Cache, '-Exe', $script:Exe, '-KeepCache')
    if ($script:LocalArt) { $a += '-LocalArt' }
    # Made by this script before the steps marked their packs: still its own.
    if (Test-Recorded 'dir' $script:Pack) { $a += '-Ours' }
    if ($script:Dry) {
        if (Test-Path -LiteralPath $steps) { $null = Invoke-Step $steps ($a + '-DryRun') }
        else { Write-Action "steps.ps1 $(Format-Command $a)" }
        return
    }
    if (-not (Test-Path -LiteralPath $steps)) { Stop-Install "her source ($(Get-Tilde $src)) has no packaging\windows\steps.ps1 (older than this installer): run this again with --from-source" }
    New-RecordedDir $script:Cache
    $null = Invoke-Step $steps $a
    Add-Receipt @('dir', $script:Pack)
}

# ---- 4. the daemon and her session --------------------------------------------------------------

function Start-Daemon {
    Write-Step 'Synaps daemon and her session'
    # Now, and at sign-in through conhost --headless (no console window): steps.ps1 daemon. A Run value
    # of his own that starts it is left as it is. An EDR may flag that pattern: docs/WINDOWS.md
    # ("conhost --headless and security software").
    $a = @('daemon', '-Synaps', $script:Synaps)
    if ($script:Dry) { $a += '-DryRun' }
    $r = Invoke-Step (Get-StepScript) $a -NoStop
    if ($r['runvalue'] -eq 'set') { Add-Receipt @('reg', $script:RunKey, 'synaps-daemon') }
    if ($r['brain']) { $script:Brain = ($r['brain'] -eq 'yes') }
    $script:DaemonHow = 'at sign-in (Run key)'
    if ($script:Dry) { $script:DaemonState = '(dry run)'; return }
    if ($r['daemon'] -match '^running (\S+)') {
        $script:DaemonState = "running (protocol $($Matches[1]))"
        Write-Info 'her session: `capri`, attached if live, else resumed, else made when she starts'
    } else { $script:DaemonState = 'not running' }
}

# ---- 5. summon key / 6. autostart / 7. voice and watch -----------------------------------------

function Add-SummonKey {
    Write-Step 'Summon key'
    Write-Good 'Win+Alt+C: she registers it herself (a tap opens the Line beside her)'
    if ($script:Voice) { Write-Info 'Win+Alt+Ctrl+C toggles her mic; [voice] hold_to_talk makes Win+Alt+C push-to-talk' }
}

function Set-Autostart {
    Write-Step 'Autostart'
    if (-not $script:Autostart) { Write-Info 'skipped (--no-autostart)'; $script:AutostartHow = 'off'; return }
    Write-Info 'she starts at sign-in: a Run value (--no-autostart to skip)'
    Set-RunValue 'synaps-avatar' "`"$($script:Exe)`""
    $script:AutostartHow = 'at sign-in (Run key)'
}

function Install-Extras {
    Write-Step 'Voice and watch'
    if (-not $script:Voice -and -not $script:Watch) { Write-Info 'skipped (--voice, --watch)'; return }
    if ($script:Watch) { Write-Info 'capri-watch is Linux-only (systemd units, statvfs): skipped' }
    if (-not $script:Voice) { return }
    $rel = "https://github.com/$($script:M.repo)/releases/latest/download"
    $sum = $null
    if (-not $script:FromSource) { try { $sum = ((Get-Text "$rel/$($script:M.asset_windows_voice).sha256") -split '\s+')[0] } catch { $sum = $null } }
    if ($sum -match '^[0-9a-f]{64}$') {
        # Her voice installer, sha256 checked, its models, and her settings through her own writer:
        # steps.ps1 voice (her installer's Voice page runs the same).
        $null = Invoke-Step (Get-StepScript) @(@('voice', '-Exe', $script:Exe) + $(if ($script:Dry) { @('-DryRun') } else { @() }))
        Add-Receipt @('inno', $script:VoiceKey)
    } else {
        # No release: the same bundle, built here (CPython 3.13's embeddable package + wheels); then
        # steps.ps1 voice -Bundle for the models and her settings, as above.
        $src = Get-Source $script:Ref
        $out = Join-Path $script:Prefix 'voice'
        if ($script:Dry) { Write-Action "voice-bundle.ps1 -Out $out, then steps.ps1 voice -Bundle $out"; return }
        [string[]]$py = Find-Python
        if (-not $py) { Stop-Install 'the voice bundle needs a CPython 3.13 x64 to run pip' }
        $ps = (Get-Process -Id $PID).Path
        Invoke-Native $ps @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $src 'packaging\windows\voice-bundle.ps1'), '-Out', $out, '-Python', $py[0])
        Add-Receipt @('dir', $out)
        $models = Join-Path $env:LOCALAPPDATA 'capri-voice'
        if (-not (Test-Path $models)) { Add-Receipt @('dir', $models) }
        $null = Invoke-Step (Get-StepScript) @('voice', '-Bundle', $out, '-Exe', $script:Exe)
    }
    $script:VoiceHow = 'capri-voice (she starts it while [voice] enabled is on)'
}

function Get-SynapsHome { if ($env:SYNAPS_BASE_DIR) { $env:SYNAPS_BASE_DIR } else { Join-Path $env:USERPROFILE '.synaps-cli' } }

# Her brain: she talks through Synaps, and Synaps needs a model. Signing in is hers to walk him
# through, not a picker here: until Synaps has one her tag reads NO BRAIN YET, and her answer to
# his first message ends on the command (a chip; a click copies it).
function Invoke-Brain {
    # $script:Brain: a login or a local model, as steps 1 and 4 read it (steps.ps1).
    if ($script:Brain) {
        if (-not $script:SynapsNew) { Write-Info 'she found your Synaps. her session is `capri`: it''ll show up in /sessions.' }
        return
    }
    Write-Info 'her brain: Synaps has no model yet. say hi to her: she''ll tell you how to give her one.'
}

# A log's text from byte From on (all of it when it started over), read beside the writer that has
# it open; '' when it can't be read.
function Read-LogSince([string]$Path, [long]$From) {
    if (-not (Test-Path -LiteralPath $Path)) { return '' }
    try {
        $fs = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
        try {
            if ($fs.Length -lt $From) { $From = 0 }
            [void]$fs.Seek($From, [IO.SeekOrigin]::Begin)
            return [IO.StreamReader]::new($fs, [Text.Encoding]::UTF8).ReadToEnd()
        } finally { $fs.Dispose() }
    } catch { return '' }
}

# Her, now: the first launch is the install's, not the next sign-in's (with --no-autostart too: that
# one is about sign-in). Without a console she logs to synaps-avatar.log beside her state: up once it
# says her first frame is painted. Only in his desktop session, never over ssh. One that runs
# already as the Start menu starts her is the old binary: stopped, then started again; one he
# started with his own arguments is left be (or, stopped for her binary, not replaced by a bare
# one). Started from his profile folder. Sets $script:Started when she's up.
function Start-HerNow {
    $script:Started = $false
    if ($script:Dry) { Write-Action "Start-Process `"$(Get-Tilde $script:Exe)`""; return }
    $later = if ($script:StoppedElsewhere) { 'her run on your desktop was stopped for her new binary: start her there again (the Start menu)' }
    elseif ($script:Autostart) { 'she starts at your next sign-in, or from the Start menu' } else { 'start her from the Start menu' }
    if ($env:SSH_CONNECTION -or $env:SSH_TTY) { Write-Info "an ssh session (she would start where you can't see her, and close with it): not started here. $later"; return }
    if (-not [Environment]::UserInteractive) { Write-Info "no desktop session here: $later"; return }
    $his = @(Get-HerRuns | Where-Object { $_.Line } | Select-Object -First 1)
    if ($his) { Write-Info "she's running as you started her ($($his[0].Line)): left be. to run the new one, quit her (her menu: Quit), then start her"; return }
    if ($script:HisRun) { Write-Info "your own run of her was stopped to replace her binary: start it again: & $($script:HisRun)"; return }
    Stop-Her 'she is started again'
    $log = Join-Path $script:Data 'synaps-avatar.log'
    $from = if (Test-Path -LiteralPath $log) { (Get-Item -LiteralPath $log).Length } else { 0 }
    Write-Action "Start-Process `"$(Get-Tilde $script:Exe)`""
    $ErrorActionPreference = 'Continue'
    try { $p = Start-Process -FilePath $script:Exe -WorkingDirectory $env:USERPROFILE -PassThru }
    catch { Write-Warn "she didn't start: $($_.Exception.Message)"; return }
    # Up to 20 s (a cold start reads her pack): the lines she wrote since.
    $new = ''
    $ms = ''
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 20) {
        $new = Read-LogSince $log $from
        $m = [regex]::Match($new, 'first-frame: painted (\d+) ms')
        if ($m.Success) { $script:Started = $true; $ms = $m.Groups[1].Value; break }
        if ($p.HasExited) { $new = Read-LogSince $log $from; break }
        Start-Sleep -Milliseconds 200
    }
    if ($script:Started) { Write-Good "she's up ($ms ms to her first frame)" }
    elseif (-not $p.HasExited) { Write-Info "she's still starting: her log is $(Get-Tilde $log)" }
    else {
        Write-Warn "she didn't start (exit $($p.ExitCode)). the end of her log ($(Get-Tilde $log)):"
        $new -split "`r?`n" | Where-Object { $_ } | Select-Object -Last 4 | ForEach-Object { Write-Host "       | $_" -ForegroundColor DarkGray }
    }
}

# What stays of Synaps after --uninstall, where his login lives, and how to remove it too.
# Mine: the synaps this script installed (there was none before); PathEdit: whether its installer put
# its folder on his user Path. A Synaps he had (or one it upgraded) is his: only a pointer.
function Write-SynapsStays([string]$Mine, [string]$PathEdit, [string]$Rustup) {
    $s = if ($Mine) { $Mine } else { (Get-Command synaps -ErrorAction SilentlyContinue | Select-Object -First 1).Source }
    $cargoSynaps = Join-Path $script:CargoBin 'synaps.exe'
    if (-not $s -and (Test-Path -LiteralPath $cargoSynaps)) { $s = $cargoSynaps }
    $theirs = Join-Path $(if ($env:XDG_CONFIG_HOME) { $env:XDG_CONFIG_HOME } else { $env:LOCALAPPDATA }) 'synaps'
    $h = Get-Tilde (Get-SynapsHome)
    if ($s) {
        Write-Info "synaps stays installed (her daemon's other clients may use it): $(Get-Tilde $s)"
        Write-Info "your Synaps login and sessions live in $h"
        if ($Mine) {
            Write-Info "to remove Synaps too (this installed it): & `"$(Get-Tilde $s)`" daemon stop, then delete $(Get-Tilde $s) and $(Get-Tilde $theirs) ($h only if you want your login and sessions gone)"
            if ($PathEdit -ne 'no') { Write-Info "  its installer also put $(Get-Tilde (Split-Path -Parent $s)) on your user Path (keep it if you use Rust)" }
        } else {
            Write-Info 'to remove Synaps too: synaps daemon stop, then uninstall it the way you installed it'
        }
    }
    if ($Rustup) { Write-Info 'rustup stays too (this installed it for her build): `rustup self uninstall` removes it' }
}

function Write-Summary {
    Write-Host ''
    Write-Host " $($Bar)installed" -ForegroundColor Red -NoNewline
    if ($script:Dry) { Write-Host '  (dry run: nothing was)' -ForegroundColor Red } else { Write-Host '' }
    $rows = [ordered]@{
        'synaps'        = "$($script:SynapsV)  $(Get-Tilde $script:Synaps)"
        'synaps-avatar' = "$($script:BinVer)  $(Get-Tilde $script:Exe)"
        'her pack'      = "capri (CapriCore v1.0.4 + lachy$(if ($script:LocalArt) { ' + mochi, local only' }))  $(Get-Tilde $script:Pack)"
        'daemon'        = "$($script:DaemonState), $($script:DaemonHow)"
        'summon'        = 'Win+Alt+C'
        'autostart'     = $script:AutostartHow
    }
    if ($script:VoiceHow) { $rows['voice'] = $script:VoiceHow }
    foreach ($k in $rows.Keys) { Write-Host ('   {0,-14} {1}' -f $k, $rows[$k]) }
    Invoke-Brain
    Write-Host ''
    Start-HerNow
    Write-Host ''
    if ($script:Started) {
        Write-Host '   she''s on your screen: ' -ForegroundColor White -NoNewline
        Write-Host 'tap Win+Alt+C (or click her) to talk' -ForegroundColor DarkGray
        Write-Info "start her again: & `"$(Get-Tilde $script:Exe)`", or from the Start menu"
    } else {
        Write-Host '   start her: ' -ForegroundColor White -NoNewline
        Write-Host "& `"$($script:Exe)`"" -NoNewline
        Write-Host '      then tap Win+Alt+C (or click her) to talk' -ForegroundColor DarkGray
    }
    Write-Info "remove her: $(Get-SelfCommand) --uninstall   (synaps stays)"
}

# This script as he'd run it again with flags: the checkout's file, else the piped form (iex can't
# pass flags; a scriptblock can).
function Get-SelfCommand {
    if ($script:Checkout) { return "& `"$(Join-Path $script:Checkout 'install.ps1')`"" }
    $ref = if ($script:Ref) { $script:Ref } else { 'main' }
    return "& ([scriptblock]::Create((irm https://raw.githubusercontent.com/$RepoDefault/$ref/install.ps1)))"
}

# ---- uninstall --------------------------------------------------------------------------------

# --purge: her own files in her folders, and only those (another pack, his own files stay). Her pack
# isn't one: the receipt's `dir` entry removes it when this script made it, never otherwise.
function Remove-Hers {
    foreach ($f in @((Join-Path $script:Conf 'settings.toml'), (Join-Path $script:Conf 'settings.toml.tmp'),
            (Join-Path $script:Data 'state.json'), (Join-Path $script:Data 'state.json.tmp'),
            (Join-Path $script:Data 'sessions.json'), (Join-Path $script:Data 'sessions.json.tmp'),
            (Join-Path $script:Data 'synaps-avatar.log'), (Join-Path $script:Data 'synaps-avatar.old.log'))) {
        if (Test-Path -LiteralPath $f) { Invoke-Action "Remove-Item $f" { Remove-Item -Force -LiteralPath $f } }
    }
    if (Test-Path -LiteralPath $script:Cache) { Invoke-Action "Remove-Item -Recurse $($script:Cache)" { Remove-Item -Recurse -Force -LiteralPath $script:Cache } }
}

function Invoke-Uninstall {
    $script:StepCount = 1
    Write-Step 'Remove what install.ps1 installed'
    $rmdirs = @()
    $innoRan = $false
    $synapsMine = Get-ReceiptMeta 'synaps'
    $synapsPathMine = Get-ReceiptMeta 'synaps_path'
    $rustupMine = Get-ReceiptMeta 'rustup'
    if (Test-Path $script:Receipt) {
        Stop-Her 'she is removed'
        $lines = @(Get-Content -LiteralPath $script:Receipt)
        [array]::Reverse($lines)
        foreach ($line in $lines) {
            if (-not $line) { continue }
            $f = $line -split "`t"
            $skip = { param($What) Write-Warn "left alone: $What (not hers: the receipt is wrong there)" }
            switch ($f[0]) {
                'inno' {
                    if ($f[1] -notin @($script:InnoKey, $script:VoiceKey)) { & $skip $f[1] }
                    else {
                        $u = (Get-ItemProperty -Path $f[1] -ErrorAction SilentlyContinue).UninstallString
                        if ($u -and $f[1] -eq $script:InnoKey) { $innoRan = $true }
                        if ($u) {
                            $exe = $u.Trim('"')
                            Write-Action "$exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART"
                            if (-not $script:Dry) {
                                $p = Start-Process -FilePath $exe -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
                                if ($p.ExitCode -ne 0) { Write-Warn "its uninstaller exited $($p.ExitCode)" }
                                # Inno's uninstaller hands off to a copy of itself: wait for that one too.
                                for ($i = 0; $i -lt 60 -and (Test-Path $f[1]); $i++) { Start-Sleep -Milliseconds 500 }
                            }
                        }
                    }
                }
                'reg' {
                    if ($f[1] -ne $script:RunKey -or $f[2] -notin @('synaps-avatar', 'synaps-daemon')) { & $skip "$($f[1]) $($f[2])" }
                    elseif ((Get-ItemProperty -Path $f[1] -Name $f[2] -ErrorAction SilentlyContinue)) {
                        Invoke-Action "reg delete $($f[1]) /v $($f[2])" { Remove-ItemProperty -Path $f[1] -Name $f[2] }
                    }
                }
                'regkey' {
                    if ($f[1] -ne $script:Aumid) { & $skip $f[1] }
                    elseif (Test-Path $f[1]) { Invoke-Action "reg delete $($f[1])" { Remove-Item -Recurse -Force $f[1] } }
                }
                'file' {
                    if (-not (Test-Removable $f[1] 'file')) { & $skip $f[1] }
                    elseif (Test-Path -LiteralPath $f[1]) { Invoke-Action "Remove-Item $($f[1])" { Remove-Item -Force -LiteralPath $f[1] } }
                }
                'dir' {
                    if (-not (Test-Removable $f[1] 'dir')) { & $skip $f[1] }
                    elseif (Test-Path -LiteralPath $f[1]) { Invoke-Action "Remove-Item -Recurse $($f[1])" { Remove-Item -Recurse -Force -LiteralPath $f[1] } }
                }
                'rmdir' { if (Test-Removable $f[1] 'rmdir') { $rmdirs += $f[1] } else { & $skip $f[1] } }
                { $_ -in 'prefix', 'meta' } { }
                default { Write-Warn "a receipt entry this script doesn't know (left alone): $line" }
            }
        }
        Invoke-Action "Remove-Item $($script:Receipt)" { Remove-Item -Force -LiteralPath $script:Receipt }
    } else {
        Write-Info "no receipt ($(Get-Tilde $script:Receipt)): nothing installed by this script"
    }
    Get-ChildItem -Directory -Path (Split-Path -Parent $script:Pack) -Filter "$(Split-Path -Leaf $script:Pack).pre-install-*" -ErrorAction SilentlyContinue |
        ForEach-Object { Write-Info "kept: the pack that was there before she was installed, $(Get-Tilde $_.FullName) (yours)" }
    if ($script:Purge) { Remove-Hers }
    elseif ($innoRan) { Write-Info "kept: your settings ($(Get-Tilde $script:Conf)); her installer's uninstaller took her state and log; --purge removes the settings too" }
    else { Write-Info "kept: your settings ($(Get-Tilde $script:Conf)) and her state and log ($(Get-Tilde $script:Data)); --purge removes them" }
    # Her folders if they're empty now, then the ones this made (innermost first) if they are.
    foreach ($d in (@($script:Conf, (Split-Path -Parent $script:Pack), $script:Data) + $rmdirs)) {
        if ((Test-Path -LiteralPath $d) -and -not (Get-ChildItem -Force -LiteralPath $d)) { Invoke-Action "rmdir $d" { Remove-Item -Force -LiteralPath $d } }
    }
    if ($script:Purge -and -not $script:Dry) {
        foreach ($d in @($script:Conf, $script:Data)) {
            if (Test-Path -LiteralPath $d) { Write-Info "left in $(Get-Tilde $d), not hers: $((Get-ChildItem -Force -LiteralPath $d | Select-Object -First 5 | ForEach-Object { $_.Name }) -join ' ')" }
        }
    }
    Write-SynapsStays $synapsMine $synapsPathMine $rustupMine
    Write-Host ''
    Write-Host " $($Bar)removed" -ForegroundColor Red -NoNewline
    if ($script:Dry) { Write-Host '  (dry run: nothing was)' -ForegroundColor Red } else { Write-Host '' }
}

function Invoke-CapriInstall([string[]]$ArgList) {
    # Here, not at the top: under `irm | iex` the top is the caller's scope.
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    # Read once, said in the banner, then cleared: under `irm | iex` this is his session, and a flag
    # left there (--uninstall) would apply to his next bare `irm | iex` too.
    $script:FlagsEnv = ''
    if ($env:CAPRI_INSTALL_FLAGS) {
        if (-not $ArgList -or $ArgList.Count -eq 0) {
            $ArgList = @($env:CAPRI_INSTALL_FLAGS -split '\s+' | Where-Object { $_ })
            $script:FlagsEnv = "used:$($ArgList -join ' ')"
        } else { $script:FlagsEnv = "ignored:$($env:CAPRI_INSTALL_FLAGS)" }
        Remove-Item Env:\CAPRI_INSTALL_FLAGS -ErrorAction SilentlyContinue
    }
    $script:StepN = 0
    $script:Checkout = ''
    $script:SynapsNew = 0
    $script:Brain = $false
    $script:StepScript = ''
    $script:StepCount = $Steps
    $script:HelpOnly = $false
    Read-Flags $ArgList
    if ($script:HelpOnly) { return }
    $script:Tmp = Join-Path ([IO.Path]::GetTempPath()) "capri-install-$PID"
    New-Item -ItemType Directory -Force $script:Tmp | Out-Null
    try {
        Set-Paths
        if ($script:Uninstall) {
            # Uninstalling needs no network (no manifest), and works where the installs went.
            $script:Prefixes = @()
            if (Test-Path $script:Receipt) {
                $script:Prefixes = @(Get-Content -LiteralPath $script:Receipt | Where-Object { $_ -like "prefix`t*" } | ForEach-Object { ($_ -split "`t", 2)[1] } |
                        Where-Object { $_ -and [IO.Path]::IsPathRooted($_) })
            }
            if ($script:Prefixes.Count) {
                $last = $script:Prefixes[-1]
                if ($last -ne $script:Prefix) {
                    if ($script:PrefixGiven) { Write-Warn "--prefix $($script:Prefix): the receipt says she went to $last; she is removed from there" }
                    $script:Prefix = $last
                    Set-Paths
                }
            } else { $script:Prefixes = @($script:Prefix) }
            Write-Banner
            Invoke-Uninstall
            return
        }
        Read-Manifest
        Write-Banner
        if (-not [Environment]::Is64BitOperatingSystem) { Stop-Install 'she needs 64-bit Windows' }
        Add-Receipt @('prefix', $script:Prefix)
        Install-Synaps
        Install-Binary
        Install-Pack
        Start-Daemon
        Add-SummonKey
        Set-Autostart
        Install-Extras
        Write-Summary
    } finally {
        Remove-Item -Recurse -Force $script:Tmp -ErrorAction SilentlyContinue
    }
}

# A stop is said in red already (Stop-Install): no PowerShell error record after it. The module
# can't exit its caller, so it hands a marker out (below).
try { Invoke-CapriInstall $CliArgs }
catch {
    if ("$($_.Exception.Message)" -notlike 'install.ps1: *') { throw }
    [char]0 + 'capri-install-stopped'
}
} | ForEach-Object {
    # Run from a file (`-File`, `& .\install.ps1`): a stop is exit 1. Under `irm | iex` there's no
    # file, and an exit would close his window: the red line says it.
    if ("$_" -cne ([char]0 + 'capri-install-stopped')) { $_ } elseif ($PSCommandPath) { exit 1 }
}
