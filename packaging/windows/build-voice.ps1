# Builds her VOICE installer: packaging\windows\Output\synaps-avatar-voice-setup-<version>-<build>.exe.
# A separate download from hers (her installer stays ~9 MB): the voice bundle (voice-bundle.ps1)
# beside her, and the model fetch. Needs a CPython 3.13 x64 (for pip) and Inno Setup 6: both on
# GitHub's windows-latest (setup-python). Run from anywhere.
#
#   .\packaging\windows\build-voice.ps1 [-Python python] [-NoBundle]   # -NoBundle: reuse build\voice
param([string]$Python = 'python', [switch]$NoBundle)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$root = (Resolve-Path (Join-Path $here '..\..')).Path
$bundle = Join-Path $here 'build\voice'
if (-not $NoBundle) {
    & (Join-Path $here 'voice-bundle.ps1') -Out $bundle -Python $Python
}
if (-not (Test-Path (Join-Path $bundle 'python\python.exe'))) { throw "no bundle at $bundle" }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$build = (git -C $root rev-parse --short HEAD 2>$null)
if (-not $build) { $build = 'local' }
$iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) { $iscc = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe' }
if (-not (Test-Path $iscc)) { throw 'Inno Setup 6 (ISCC.exe) is not installed' }
& $iscc /Qp "/DAppVersion=$version" "/DBuild=$build" "/DBundle=$bundle" (Join-Path $here 'synaps-avatar-voice.iss')
if ($LASTEXITCODE) { throw "ISCC failed ($LASTEXITCODE)" }
$setup = Get-ChildItem (Join-Path $here 'Output') -Filter "synaps-avatar-voice-setup-$version-$build.exe"
Write-Output "voice installer: $($setup.FullName) ($([math]::Round($setup.Length / 1MB, 1)) MB)"
