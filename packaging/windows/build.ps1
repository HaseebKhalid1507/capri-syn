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

# The DLLs her exe imports (dumpbin /dependents, from the MSVC tools that built it): none a clean
# Windows lacks. The C runtime is linked in (.cargo/config.toml: +crt-static), so no VCRUNTIME140.dll
# or MSVCP140.dll (the Visual C++ Redistributable) and no UCRT forwarders (api-ms-win-crt-*).
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$dumpbin = if (Test-Path $vswhere) { & $vswhere -latest -products * -find 'VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1 }
if (-not $dumpbin) {
    if (-not $NoCargo) { throw "no dumpbin.exe (MSVC's tools, found through $vswhere): her exe's imports can't be checked" }
    Write-Warning 'no dumpbin.exe: her exe''s imports not checked'
} else {
    $deps = @(& $dumpbin /nologo /dependents $exe | ForEach-Object { if ($_ -match '^\s+(\S+\.dll)\s*$') { $Matches[1] } })
    if ($LASTEXITCODE -or -not $deps) { throw "dumpbin /dependents $exe failed ($LASTEXITCODE)" }
    Write-Output "her exe imports: $($deps -join ', ')"
    $crt = @($deps | Where-Object { $_ -match '^(vcruntime|msvcp|vcomp|concrt|ucrtbase)\d*(_\d+)?\.dll$|^api-ms-win-crt-' })
    if ($crt) {
        $what = "synaps-avatar.exe imports the C runtime ($($crt -join ', ')): a PC without the Visual C++ Redistributable can't start her (is +crt-static in .cargo/config.toml?)"
        if (-not $NoCargo) { throw $what }
        Write-Warning $what
    }
}

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
