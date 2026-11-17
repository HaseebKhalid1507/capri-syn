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

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$RepoDefault = 'HaseebKhalid1507/capri-syn'
$Steps = 7
$Bar = [string][char]0x258C
$Tick = [string][char]0x2713
$Cross = [string][char]0x2717

$Usage = @'
usage: install.ps1 [--yes] [--dry-run] [--uninstall [--purge]] [--voice] [--watch] [--no-autostart]
                   [--from-source] [--prefix DIR] [--with-local-art] [--artifact FILE]

  --yes             don't ask: Visual Studio's C++ build tools (admin) for a source build, Python
                    from winget for her pack
  --dry-run         print every action, touch nothing
  --uninstall       remove what this script installed; your settings stay
  --purge           with --uninstall: remove those too
  --voice           also her voice: the voice installer (or bundle), ~320 MB of models
  --watch           capri-watch (Linux only: skipped here)
  --no-autostart    don't start her at sign-in
  --from-source     build her from source even when a release has an installer
  --prefix DIR      where she goes (default %LOCALAPPDATA%\Programs\synaps-avatar)
  --with-local-art  also mochi's art: LOCAL PERSONAL USE ONLY, never publish or share it
  --artifact FILE   install this release asset (her setup .exe) instead of downloading one

Each flag also has PowerShell's spelling: -Yes, -DryRun, -Uninstall, -Purge, -Voice, -Watch,
-NoAutostart, -FromSource, -Prefix DIR, -WithLocalArt, -Artifact FILE.
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
    # A native program's stderr (progress, warnings) is not an error: 5.1 would throw on it.
    $ErrorActionPreference = 'Continue'
    & $Exe @ArgList
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
function Get-SynapsVersion([string]$Exe) {
    $ErrorActionPreference = 'Continue'
    try { $o = (& $Exe --version 2>$null) | Select-Object -First 1 } catch { return $null }
    if ($o -match '(\d+\.\d+\.\d+)') { return $Matches[1] } else { return $null }
}

# The receipt: what this script installed (kind, then values, tab-separated); --uninstall reads it backwards.
function Add-Receipt([string[]]$Fields) {
    if ($script:Dry) { return }
    $line = $Fields -join "`t"
    New-Item -ItemType Directory -Force (Split-Path -Parent $script:Receipt) | Out-Null
    if (-not (Test-Path $script:Receipt) -or -not (Select-String -LiteralPath $script:Receipt -SimpleMatch -Pattern $line -Quiet)) {
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
function New-RecordedDir([string]$Path) {
    if (-not (Test-Path $Path)) {
        Invoke-Action "mkdir $Path" { New-Item -ItemType Directory -Force $Path | Out-Null }
        Add-Receipt @('dir', $Path)
    }
}
function Set-RunValue([string]$Name, [string]$Value) {
    $run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    $cur = (Get-ItemProperty -Path $run -Name $Name -ErrorAction SilentlyContinue).$Name
    if ($cur -eq $Value) { Write-Good "already set: Run\$Name"; return }
    Invoke-Action "reg add HKCU\...\Run /v $Name /d $Value" { New-ItemProperty -Path $run -Name $Name -Value $Value -PropertyType String -Force | Out-Null }
    Add-Receipt @('reg', $run, $Name)
}

# ---- setup ------------------------------------------------------------------------------------

function Read-Flags([string[]]$ArgList) {
    $script:Yes = $false; $script:Dry = $false; $script:Uninstall = $false; $script:Purge = $false
    $script:Voice = $false; $script:Watch = $false; $script:Autostart = $true; $script:FromSource = $false
    $script:LocalArt = $false; $script:Prefix = Join-Path $env:LOCALAPPDATA 'Programs\synaps-avatar'; $script:Artifact = ''
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
            { $_ -in 'prefix', 'artifact' } {
                if ($null -eq $val) {
                    $i++
                    if ($i -ge $ArgList.Count) { Stop-Install "--$k needs a value" }
                    $val = [string]$ArgList[$i]
                }
                if ($k -eq 'prefix') { $script:Prefix = $val } else { $script:Artifact = $val }
            }
            { $_ -in 'h', 'help' } { Write-Output $Usage; $script:HelpOnly = $true; return }
            default { Write-Host $Usage; Stop-Install "unknown flag: $a" }
        }
        $i++
    }
    if ($script:Purge -and -not $script:Uninstall) { Stop-Install '--purge goes with --uninstall' }
    $script:Prefix = [IO.Path]::GetFullPath($script:Prefix)
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
    foreach ($k in 'repo', 'synaps_repo', 'synaps_min', 'synaps_protocol', 'synaps_pin', 'synaps_installer_ps1_sha256',
        'asset_windows', 'asset_windows_voice', 'rustup_dist', 'capricore_repo', 'capricore_commit', 'pip_pack') {
        if (-not $m[$k]) { Stop-Install "manifest: no $k" }
    }
    $script:M = $m
}

function Set-Paths {
    $script:Data = Join-Path $env:LOCALAPPDATA 'synaps-avatar'
    $script:Conf = Join-Path $env:APPDATA 'synaps-avatar'
    $script:Pack = Join-Path $script:Data 'packs\capri'
    $script:Cache = Join-Path $script:Data 'installer'
    $script:Venv = Join-Path $script:Cache 'venv'
    $script:Receipt = Join-Path $script:Data 'install.receipt'
    $script:Exe = Join-Path $script:Prefix 'synaps-avatar.exe'
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
}

# Her source tree: this checkout, else GitHub's tarball of a ref (her make-pack and its pins, and
# the code for a source build).
function Get-Source([string]$Ref) {
    if ($script:Checkout) { return $script:Checkout }
    if ($script:SrcDir) { return $script:SrcDir }
    $sha = $null
    try { $sha = (Get-Text "https://api.github.com/repos/$($script:M.repo)/commits/$Ref" (Get-Headers + @{ Accept = 'application/vnd.github.sha' })).Trim() } catch { $sha = $null }
    if ($sha -notmatch '^[0-9a-f]{40}$') { $sha = $Ref }
    $short = if ($sha.Length -ge 12) { $sha.Substring(0, 12) } else { $sha }
    $dir = Join-Path $script:Cache "src\capri-syn-$short"
    Write-Info "her files: $($script:M.repo) @ $short ($Ref)"
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

# ---- 1. synaps --------------------------------------------------------------------------------

function Install-Synaps {
    Write-Step 'Synaps'
    $cur = (Get-Command synaps -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    $mine = Join-Path $script:CargoBin 'synaps.exe'
    if (-not $cur -and (Test-Path $mine)) { $cur = $mine }
    if ($cur) {
        $v = Get-SynapsVersion $cur
        if ($v -and ([version]$v -ge [version]$script:M.synaps_min)) {
            $script:Synaps = $cur; $script:SynapsV = $v
            Write-Good "synaps $v ($(Get-Tilde $cur))"
            return
        }
        Write-Info "synaps $v at $(Get-Tilde $cur) is older than $($script:M.synaps_min): installing the latest"
    } else {
        Write-Info 'no synaps: installing the latest with its own installer'
    }
    # The latest release, and GitHub's sha256 of its installer; else the manifest's pin.
    $tag = $null; $sha = $null
    try {
        $rel = Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$($script:M.synaps_repo)/releases/latest" -Headers (Get-Headers)
        $tag = $rel.tag_name
        $asset = $rel.assets | Where-Object { $_.name -eq 'synaps-installer.ps1' } | Select-Object -First 1
        if ($asset -and $asset.digest) { $sha = ([string]$asset.digest) -replace '^sha256:', '' }
    } catch { $tag = $null }
    if (-not $tag -or $sha -notmatch '^[0-9a-f]{64}$') {
        $tag = "v$($script:M.synaps_pin)"; $sha = $script:M.synaps_installer_ps1_sha256
        Write-Info "GitHub's API didn't answer: the pinned $tag"
    }
    $file = Join-Path $script:Tmp 'synaps-installer.ps1'
    Get-Download "https://github.com/$($script:M.synaps_repo)/releases/download/$tag/synaps-installer.ps1" $file
    Test-Sha256 $file $sha
    $ps = (Get-Process -Id $PID).Path
    Invoke-Native $ps @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $file)
    $script:Synaps = $mine; $script:SynapsV = $tag.TrimStart('v')
    if ($env:Path -notlike "*$($script:CargoBin)*") { $env:Path = "$($script:CargoBin);$env:Path" }
    if (-not $script:Dry) {
        if (-not (Test-Path $mine)) { Stop-Install "the synaps installer ran, but there's no $mine" }
        $script:SynapsV = Get-SynapsVersion $mine
        Write-Good "synaps $($script:SynapsV) ($(Get-Tilde $mine))"
    }
    Set-ReceiptMeta 'synaps' $mine
}

# ---- 2. her binary ----------------------------------------------------------------------------

function Stop-Her {
    if ($script:Dry) { return }
    $p = Get-Process -Name 'synaps-avatar' -ErrorAction SilentlyContinue
    if ($p) { Invoke-Action 'Stop-Process -Name synaps-avatar (she is replaced)' { $p | Stop-Process -Force; Start-Sleep -Milliseconds 500 } }
}

# Her installer (packaging/windows), unattended; her autostart is step 6's, not its task.
function Install-Setup([string]$Setup) {
    Stop-Her
    New-RecordedDir $script:Cache
    $log = Join-Path $script:Cache 'setup.log'
    $argLine = "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /DIR=`"$($script:Prefix)`" /MERGETASKS=`"!autostart`" /LOG=`"$log`""
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

function Find-VCTools {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $null }
    $p = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($p) { return [string]$p } else { return $null }
}

function Build-FromSource {
    $src = Get-Source $script:Ref
    if (-not (Find-VCTools)) {
        $wg = @('install', '--id', 'Microsoft.VisualStudio.2022.BuildTools', '-e', '--accept-package-agreements', '--accept-source-agreements',
            '--override', '--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
        Write-Info "Rust on Windows links with Visual Studio's C++ build tools (not installed; ~3 GB, admin):"
        Write-Info "  winget $(Format-Command $wg)"
        if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
            if ($script:Dry) { Write-Warn 'no winget: a real run stops here (install the C++ build tools by hand)' }
            else { Stop-Install 'no winget: install the C++ build tools from https://visualstudio.microsoft.com/visual-cpp-build-tools/, then run this again' }
        }
        if (-not (Confirm-Action 'install them with winget (a UAC prompt)')) { Stop-Install 'run that (or this script with --yes), then run this again' }
        Invoke-Native 'winget' $wg
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
        $t0 = Get-Date
        Invoke-Native $cargo @('rustc', '--locked', '--release', '--bin', 'synaps-avatar', '--', '-C', 'link-arg=/SUBSYSTEM:WINDOWS', '-C', 'link-arg=/ENTRY:mainCRTStartup')
        if (-not $script:Dry) { Write-Good "built in $([int]((Get-Date) - $t0).TotalMinutes) min" }
    } finally {
        if (-not $script:Dry) { Pop-Location }
        Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    }
    Stop-Her
    New-RecordedDir $script:Prefix
    $built = Join-Path $target 'release\synaps-avatar.exe'
    Invoke-Action "copy $built $($script:Exe)" { Copy-Item -Force $built $script:Exe }
    Add-Receipt @('file', $script:Exe)
    # What her installer does besides: her AUMID (toasts under her name) and a Start-menu shortcut.
    $aumid = 'HKCU:\Software\Classes\AppUserModelId\Synaps.Avatar'
    if (-not (Test-Path $aumid)) {
        Invoke-Action "reg add HKCU\Software\Classes\AppUserModelId\Synaps.Avatar /v DisplayName /d synaps-avatar" {
            New-Item -Force $aumid | Out-Null
            New-ItemProperty -Path $aumid -Name DisplayName -Value 'synaps-avatar' -PropertyType String -Force | Out-Null
        }
        Add-Receipt @('regkey', $aumid)
    }
    $lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\synaps-avatar.lnk'
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

function Show-Notices {
    $l = Join-Path $script:Pack 'anim\lachy\LICENSE.md'
    if (-not (Test-Path $l)) { return }
    Write-Info "lachy's desktop art: PolyForm Noncommercial 1.0.0 (https://polyformproject.org/licenses/noncommercial/1.0.0)"
    Select-String -LiteralPath $l -Pattern '^Required Notice:' | ForEach-Object { Write-Host "     $($_.Line)" -ForegroundColor White }
}

# Her --check: the windowed exe writes to her log, not to a console.
function Test-Pack {
    $log = Join-Path $script:Data 'synaps-avatar.log'
    $before = if (Test-Path $log) { (Get-Item $log).Length } else { 0 }
    $p = Start-Process -FilePath $script:Exe -ArgumentList "--pack `"$($script:Pack)`" --check" -Wait -PassThru -WindowStyle Hidden
    $text = ''
    if (Test-Path $log) {
        $fs = [IO.File]::Open($log, 'Open', 'Read', 'ReadWrite')
        try {
            if ($fs.Length -ge $before) { [void]$fs.Seek($before, 'Begin') }
            $text = (New-Object IO.StreamReader($fs)).ReadToEnd()
        } finally { $fs.Dispose() }
    }
    return @{ Code = $p.ExitCode; Text = $text }
}

function Install-Pack {
    Write-Step 'Her art pack'
    [string[]]$py = Find-Python
    if (-not $py) {
        $wg = @('install', '-e', '--id', 'Python.Python.3.13', '--scope', 'user', '--accept-package-agreements', '--accept-source-agreements')
        Write-Info "her pack is assembled by a Python script (make-pack); no Python here: winget $(Format-Command $wg)"
        if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
            if ($script:Dry) { Write-Warn 'no winget: a real run stops here (install Python 3 from https://www.python.org)' }
            else { Stop-Install 'no winget: install Python 3 from https://www.python.org, then run this again' }
        }
        if (-not (Confirm-Action 'install Python 3.13 for you (no admin) with winget')) { Stop-Install 'run that (or this script with --yes), then run this again' }
        Invoke-Native 'winget' $wg
        $p313 = Join-Path $env:LOCALAPPDATA 'Programs\Python\Python313\python.exe'
        [string[]]$py = if ($script:Dry) { 'python' } elseif (Test-Path $p313) { $p313 } else { Find-Python }
        if (-not $py) { Stop-Install 'Python was installed, but this session can''t see it yet: open a new terminal and run this again' }
    }
    $vpy = Join-Path $script:Venv 'Scripts\python.exe'
    if (-not (Test-Path $vpy)) {
        New-RecordedDir $script:Cache
        $pre = @($py | Select-Object -Skip 1)
        Invoke-Native $py[0] ($pre + @('-m', 'venv', $script:Venv))
    }
    $want = @($script:M.pip_pack -split '\s+' | Where-Object { $_ })
    if ($script:LocalArt -and $script:M.pip_local_art) { $want += @($script:M.pip_local_art -split '\s+' | Where-Object { $_ }) }
    $stamp = Join-Path $script:Venv '.capri-pip'
    if ((Test-Path $stamp) -and ((Get-Content -Raw $stamp).Trim() -eq ($want -join ' '))) {
        Write-Good "make-pack's Python: $($want -join ' ')"
    } else {
        Invoke-Native $vpy (@('-m', 'pip', 'install', '-q', '--disable-pip-version-check', '--no-cache-dir') + $want)
        if (-not $script:Dry) { Set-Content -LiteralPath $stamp -Value ($want -join ' ') }
    }
    $src = Get-Source $(if ($script:ReleaseTag) { $script:ReleaseTag } else { $script:Ref })
    $cc = Join-Path $script:Cache "CapriCore-$($script:M.capricore_commit.Substring(0, 7))"
    if (-not (Test-Path (Join-Path $cc 'docs\character-assets.md'))) {
        New-RecordedDir $script:Cache
        $tgz = Join-Path $script:Tmp 'capricore.tar.gz'
        Get-Download "https://codeload.github.com/$($script:M.capricore_repo)/tar.gz/$($script:M.capricore_commit)" $tgz
        Invoke-Action "mkdir $cc" { New-Item -ItemType Directory -Force $cc | Out-Null }
        Invoke-Native 'tar.exe' @('-xzf', $tgz, '-C', $cc, '--strip-components=1')
    } else {
        Write-Good "CapriCore $($script:M.capricore_commit.Substring(0, 7)) (cached)"
    }
    # Up to date: same make-pack, pins, moods, persona and flags as last time, and it still checks.
    $parts = @(Get-ChildItem (Join-Path $src 'tools') -Filter '*.sha256' -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
    $parts += @('tools\make-pack', 'tools\desktop-art.json', 'packs\capri\moods.json', 'packs\capri\persona.md' | ForEach-Object { Join-Path $src $_ })
    # One sha256 over every part's (a dry run from GitHub has none of them: the hash of nothing).
    $joined = ($parts | Where-Object { Test-Path $_ } | ForEach-Object { Get-Sha256 $_ }) -join ''
    $sha = [Security.Cryptography.SHA256]::Create()
    $fp = (-join ($sha.ComputeHash([Text.Encoding]::ASCII.GetBytes($joined)) | ForEach-Object { $_.ToString('x2') })).Substring(0, 16) + "-$([int]$script:LocalArt)"
    if (-not $script:Dry -and (Get-ReceiptMeta 'pack') -eq $fp -and (Test-Path (Join-Path $script:Pack 'sprites.json'))) {
        $c = Test-Pack
        if ($c.Code -eq 0) { Write-Good "up to date ($(Get-Tilde $script:Pack))"; Show-Notices; return }
    }
    $side = @('--lachy', 'auto')
    if ($script:LocalArt) {
        $side += @('--mochi', 'auto')
        Write-Warn "--with-local-art: mochi's art is for LOCAL PERSONAL USE ONLY: never commit, publish or share it"
    }
    $new = "$($script:Pack).new"; $old = "$($script:Pack).old"
    Invoke-Action "Remove-Item $new" { if (Test-Path $new) { Remove-Item -Recurse -Force $new } }
    # make-pack's download cache (lachy's files) under the installer's own folder.
    $env:XDG_CACHE_HOME = Join-Path $script:Cache 'xdg'
    try { Invoke-Native $vpy (@((Join-Path $src 'tools\make-pack'), $cc, $new) + $side) }
    finally { Remove-Item Env:\XDG_CACHE_HOME -ErrorAction SilentlyContinue }
    Invoke-Action "replace $($script:Pack)" {
        if (Test-Path $old) { Remove-Item -Recurse -Force $old }
        if (Test-Path $script:Pack) { Move-Item $script:Pack $old }
        Move-Item $new $script:Pack
        if (Test-Path $old) { Remove-Item -Recurse -Force $old }
    }
    Add-Receipt @('dir', $script:Pack)
    Set-ReceiptMeta 'pack' $fp
    Show-Notices
    Write-Action "$($script:Exe) --pack $($script:Pack) --check"
    if (-not $script:Dry) {
        $c = Test-Pack
        $ok = [regex]::Match($c.Text, 'pack `[^`]*` ok: \d+ frames[^,]*').Value
        if ($c.Code -ne 0 -or -not $ok) {
            Write-Host ($c.Text -split "`n" | Select-Object -Last 5 | Out-String)
            Stop-Install "her pack doesn't check: synaps-avatar --check exited $($c.Code)"
        }
        Write-Good "$ok (--check)"
    }
}

# ---- 4. the daemon and her session --------------------------------------------------------------

function Start-Daemon {
    Write-Step 'Synaps daemon and her session'
    $ErrorActionPreference = 'Continue'
    $s = ''
    if (-not $script:Dry) { $s = (& $script:Synaps daemon status 2>&1 | Out-String); $up = ($LASTEXITCODE -eq 0) } else { $up = $false }
    if (-not $up) { Invoke-Native $script:Synaps @('daemon', '--detach') }
    # At sign-in: Synaps has no service on Windows; a Run value starts it (whether a console flashes: docs/WINDOWS.md item 15).
    $run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    $theirs = (Get-ItemProperty -Path $run -ErrorAction SilentlyContinue).PSObject.Properties |
        Where-Object { $_.Name -ne 'synaps-daemon' -and "$($_.Value)" -match 'synaps' -and "$($_.Value)" -match 'daemon' } | Select-Object -First 1
    if ($theirs) { Write-Good "your Run\$($theirs.Name) starts the daemon at sign-in" }
    else { Set-RunValue 'synaps-daemon' "`"$($script:Synaps)`" daemon --detach" }
    $script:DaemonHow = 'at sign-in (Run key)'
    if ($script:Dry) { $script:DaemonState = '(dry run)'; return }
    for ($i = 0; $i -lt 20; $i++) {
        $s = (& $script:Synaps daemon status 2>&1 | Out-String)
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($s -notmatch 'daemon: running') {
        Write-Warn "the daemon isn't answering yet: $(($s -split "`n")[0])"
        $script:DaemonState = 'not running'
        return
    }
    $p = if ($s -match 'protocol: (\d+)') { $Matches[1] } else { '?' }
    if ($p -ne '?' -and $p -ne $script:M.synaps_protocol) {
        Write-Warn "the daemon speaks protocol $p; her client speaks $($script:M.synaps_protocol) (exact match): she can't attach. Install synaps $($script:M.synaps_pin), or a capri-syn that speaks $p"
    }
    $script:DaemonState = "running (protocol $p)"
    Write-Good "daemon $($script:DaemonState): $($script:DaemonHow)"
    Write-Info 'her session: `capri`, attached if live, else resumed, else made when she starts'
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
        $setup = Join-Path $script:Tmp $script:M.asset_windows_voice
        Get-Download "$rel/$($script:M.asset_windows_voice)" $setup
        Test-Sha256 $setup $sum
        $argLine = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /MERGETASKS="models"'
        Write-Action "$(Split-Path -Leaf $setup) $argLine"
        if (-not $script:Dry) {
            $p = Start-Process -FilePath $setup -ArgumentList $argLine -Wait -PassThru
            if ($p.ExitCode -ne 0) { Stop-Install "her voice installer failed (exit $($p.ExitCode))" }
        }
        Add-Receipt @('inno', $script:VoiceKey)
    } else {
        # No release: the same bundle, built here (CPython 3.13's embeddable package + wheels).
        $src = Get-Source $script:Ref
        $out = Join-Path $script:Prefix 'voice'
        [string[]]$py = Find-Python
        if (-not $py) { Stop-Install 'the voice bundle needs a CPython 3.13 x64 to run pip' }
        $ps = (Get-Process -Id $PID).Path
        Invoke-Native $ps @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $src 'packaging\windows\voice-bundle.ps1'), '-Out', $out, '-Python', $py[0])
        Add-Receipt @('dir', $out)
        $models = Join-Path $env:LOCALAPPDATA 'capri-voice'
        if (-not (Test-Path $models)) { Add-Receipt @('dir', $models) }
        Invoke-Native $ps @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $out 'voice-models.ps1'))
    }
    $settings = Join-Path $script:Conf 'settings.toml'
    if (-not (Test-Path $settings)) {
        Invoke-Action "write $settings ([voice] enabled = true)" {
            New-Item -ItemType Directory -Force $script:Conf | Out-Null
            Set-Content -LiteralPath $settings -Value "[voice]`nenabled = true" -Encoding ASCII
        }
    } elseif (-not (Select-String -LiteralPath $settings -Pattern '^\s*enabled\s*=\s*true' -Quiet)) {
        Write-Info "turn her voice on in $(Get-Tilde $settings):  [voice] enabled = true"
    }
    $script:VoiceHow = 'capri-voice (she starts it while [voice] enabled is on)'
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
    Write-Host ''
    Write-Host '   start her: ' -ForegroundColor White -NoNewline
    Write-Host "& `"$($script:Exe)`"" -NoNewline
    Write-Host '      then tap Win+Alt+C (or click her) to talk' -ForegroundColor DarkGray
    Write-Info 'remove her: install.ps1 --uninstall   (synaps stays)'
}

# ---- uninstall --------------------------------------------------------------------------------

function Invoke-Uninstall {
    $script:StepCount = 1
    Write-Step 'Remove what install.ps1 installed'
    if (Test-Path $script:Receipt) {
        Stop-Her
        $lines = @(Get-Content -LiteralPath $script:Receipt)
        [array]::Reverse($lines)
        foreach ($line in $lines) {
            $f = $line -split "`t"
            switch ($f[0]) {
                'inno' {
                    $u = (Get-ItemProperty -Path $f[1] -ErrorAction SilentlyContinue).UninstallString
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
                'reg' {
                    if ((Get-ItemProperty -Path $f[1] -Name $f[2] -ErrorAction SilentlyContinue)) {
                        Invoke-Action "reg delete $($f[1]) /v $($f[2])" { Remove-ItemProperty -Path $f[1] -Name $f[2] }
                    }
                }
                'regkey' { if (Test-Path $f[1]) { Invoke-Action "reg delete $($f[1])" { Remove-Item -Recurse -Force $f[1] } } }
                'file' { if (Test-Path -LiteralPath $f[1]) { Invoke-Action "Remove-Item $($f[1])" { Remove-Item -Force -LiteralPath $f[1] } } }
                'dir' { if (Test-Path -LiteralPath $f[1]) { Invoke-Action "Remove-Item -Recurse $($f[1])" { Remove-Item -Recurse -Force -LiteralPath $f[1] } } }
            }
        }
        Invoke-Action "Remove-Item $($script:Receipt)" { Remove-Item -Force -LiteralPath $script:Receipt }
        foreach ($d in @((Split-Path -Parent $script:Pack), $script:Data, $script:Prefix)) {
            if ((Test-Path $d) -and -not (Get-ChildItem -Force $d)) { Invoke-Action "rmdir $d" { Remove-Item -Force $d } }
        }
    } else {
        Write-Info "no receipt ($(Get-Tilde $script:Receipt)): nothing installed by this script"
    }
    if ($script:Purge) {
        foreach ($d in @($script:Conf, $script:Data)) {
            if (Test-Path $d) { Invoke-Action "Remove-Item -Recurse $d" { Remove-Item -Recurse -Force $d } }
        }
    } else {
        Write-Info "kept: your settings ($(Get-Tilde $script:Conf)); --purge removes them, and anything left in $(Get-Tilde $script:Data)"
    }
    Write-Info 'synaps stays (her daemon''s other clients may need it); rustup too, if this installed it'
    Write-Host ''
    Write-Host " $($Bar)removed" -ForegroundColor Red -NoNewline
    if ($script:Dry) { Write-Host '  (dry run: nothing was)' -ForegroundColor Red } else { Write-Host '' }
}

function Invoke-CapriInstall([string[]]$ArgList) {
    $script:StepN = 0
    $script:StepCount = $Steps
    $script:HelpOnly = $false
    Read-Flags $ArgList
    if ($script:HelpOnly) { return }
    $script:Tmp = Join-Path ([IO.Path]::GetTempPath()) "capri-install-$PID"
    New-Item -ItemType Directory -Force $script:Tmp | Out-Null
    try {
        Read-Manifest
        Set-Paths
        Write-Banner
        if ($script:Uninstall) { Invoke-Uninstall; return }
        if (-not [Environment]::Is64BitOperatingSystem) { Stop-Install 'she needs 64-bit Windows' }
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

Invoke-CapriInstall $args
