# Builds her per-user installer: packaging\windows\Output\capri-syn-setup-<version>-<build>.exe (her
# binary, and the install steps it runs after: steps.ps1 and what it reads; no art).
# Needs the Rust toolchain (rust-toolchain.toml), Inno Setup 6 (ISCC) and ImageMagick 7 (magick):
# all three are on GitHub's windows-latest image. Run from anywhere.
#
#   .\packaging\windows\build.ps1               # build her exe, then the installer
#   .\packaging\windows\build.ps1 -NoCargo      # reuse target\release\synaps-avatar.exe as built
#
# Her exe is the windowed build (no console window from a shortcut or at sign-in; without a console
# she logs to %LOCALAPPDATA%\synaps-avatar\synaps-avatar.log). `cargo rustc` passes the subsystem to
# her binary only, so no dependency is rebuilt for it.
param([switch]$NoCargo)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$root = (Resolve-Path (Join-Path $here '..\..')).Path
$work = Join-Path $here 'build'
New-Item -ItemType Directory -Force $work | Out-Null

if (-not $NoCargo) {
    Push-Location $root
    try {
        cargo rustc --locked --release --bin synaps-avatar '--' -C link-arg=/SUBSYSTEM:WINDOWS -C link-arg=/ENTRY:mainCRTStartup
        if ($LASTEXITCODE) { throw "cargo rustc failed ($LASTEXITCODE)" }
    } finally { Pop-Location }
}
$exe = Join-Path $root 'target\release\synaps-avatar.exe'
if (-not (Test-Path $exe)) { throw "no $exe" }

# The PE header's subsystem (2: windowed, 3: console): a console build would open a console
# window from the Start menu.
$pe = [IO.File]::ReadAllBytes($exe)
$subsystem = [BitConverter]::ToUInt16($pe, [BitConverter]::ToInt32($pe, 0x3C) + 0x5C)
if (-not $NoCargo -and $subsystem -ne 2) { throw "synaps-avatar.exe is not windowed (subsystem $subsystem)" }
if ($subsystem -ne 2) { Write-Warning "synaps-avatar.exe is a console build (subsystem $subsystem): a console window will open with her" }

# Her icon: not her art (that stays in his packs), a mark in her colours: her red bolt on her black.
$png = Join-Path $work 'synaps-avatar.png'
$ico = Join-Path $work 'synaps-avatar.ico'
magick -size 256x256 xc:none `
    -fill '#0a0a0b' -stroke '#8a0f24' -strokewidth 8 -draw 'roundrectangle 8,8 247,247 52,52' `
    -fill '#ff1438' -stroke none -draw 'polygon 141,20 187,20 141,104 187,104 83,236 115,136 69,136' `
    $png
if ($LASTEXITCODE) { throw "magick (png) failed ($LASTEXITCODE)" }
magick $png -define 'icon:auto-resize=256,64,48,32,24,16' $ico
if ($LASTEXITCODE) { throw "magick (ico) failed ($LASTEXITCODE)" }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$build = (git -C $root rev-parse --short HEAD 2>$null)
if (-not $build) { $build = 'local' }
$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) { $iscc = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe' }
if (-not (Test-Path $iscc)) { throw 'Inno Setup 6 (ISCC.exe) is not installed' }
& $iscc /Qp "/DAppVersion=$version" "/DBuild=$build" "/DExe=$exe" "/DIconDir=$work" (Join-Path $here 'synaps-avatar.iss')
if ($LASTEXITCODE) { throw "ISCC failed ($LASTEXITCODE)" }
$setup = Get-ChildItem (Join-Path $here 'Output') -Filter "capri-syn-setup-$version-$build.exe"
Write-Output "installer: $($setup.FullName) ($([math]::Round($setup.Length / 1MB, 1)) MB)"
