# Builds her per-user installer: packaging\windows\Output\capri-syn-setup-<version>-<build>.exe (her
# binary, her licence and the notices that go with it, and the install steps it runs after: steps.ps1
# and what it reads; no art).
# Needs the Rust toolchain (rust-toolchain.toml) and Inno Setup 6 (ISCC): both are on GitHub's
# windows-latest image. Run from anywhere. Her icon is assets\icon\capri-syn.ico (the master), as is.
# The crates' licences are generated for this build by cargo-about (installer/licenses/third-party.ps1:
# it downloads its pinned release once, into target\).
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

# Her licence and the notices that go with her binary (installer/licenses/list), laid out for the .iss
# (under {app}), with the crates' licences for this target.
$notices = Join-Path $here 'build\notices'
if (Test-Path $notices) { Remove-Item -Recurse -Force $notices }
foreach ($line in Get-Content (Join-Path $root 'installer\licenses\list')) {
    if ($line -match '^\s*(#|$)') { continue }
    $from, $to = $line -split "`t", 2
    $dest = Join-Path $notices $to
    New-Item -ItemType Directory -Force (Split-Path -Parent $dest) | Out-Null
    if ($from -eq '@third-party@') { & (Join-Path $root 'installer\licenses\third-party.ps1') -Target 'x86_64-pc-windows-msvc' -Out $dest }
    else { Copy-Item (Join-Path $root ($from -replace '@os@', 'windows')) $dest }
}

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$build = (git -C $root rev-parse --short HEAD 2>$null)
if (-not $build) { $build = 'local' }
$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) { $iscc = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe' }
if (-not (Test-Path $iscc)) { throw 'Inno Setup 6 (ISCC.exe) is not installed' }
Write-Output "Inno Setup $((Get-Item $iscc).VersionInfo.ProductVersion) ($iscc)"
& $iscc /Qp "/DAppVersion=$version" "/DBuild=$build" "/DExe=$exe" "/DNotices=$notices" (Join-Path $here 'synaps-avatar.iss')
if ($LASTEXITCODE) { throw "ISCC failed ($LASTEXITCODE)" }
$setup = Get-ChildItem (Join-Path $here 'Output') -Filter "capri-syn-setup-$version-$build.exe"
Write-Output "installer: $($setup.FullName) ($([math]::Round($setup.Length / 1MB, 1)) MB)"
