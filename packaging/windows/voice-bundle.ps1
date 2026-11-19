# Builds capri-voice's Windows bundle: a folder that runs the sidecar with no Python installed.
# Not in her installer yet (docs/design/windows-voice.md, "Packaging"); CI builds it and runs the
# selftest and the unit tests with the bundled interpreter.
#
#   <Out>\python\          CPython's embeddable package (python.org; pinned, SHA-256 checked), its
#                          python313._pth pointing at ..\site-packages, and the VC++ runtime the
#                          wheels' DLLs import (msvcp140*.dll, vcruntime140*.dll: from Visual Studio)
#   <Out>\site-packages\   tools\capri-voice-win-requirements.txt, wheels only (nothing compiled)
#   <Out>\capri-voice, <Out>\capri_voice_win.py
#   <Out>\voice-models.ps1, <Out>\wake\hey-capri-v1.npz   (the model fetch, and the one model in the repo)
#   <Out>\LICENSE, <Out>\wake\NOTICE   capri-syn's licence; the wake-word model's, beside it
#   <Out>\licenses\       every package's licence texts, espeak-ng's and whisper.cpp's (inside two
#                          wheels that don't carry them), and where the GPL parts' source is
#                          (voice-licenses.py: SOURCE.txt). CPython's own: <Out>\python\LICENSE.txt.
#
# Run it:  <Out>\python\pythonw.exe <Out>\capri-voice      (no console; python.exe for one)
# Models are never in it: %LOCALAPPDATA%\capri-voice\models, fetched by voice-models.ps1 (what
# capri-voice-setup fetches on Linux). The voice installer (synaps-avatar-voice.iss) carries it.
#
#   .\packaging\windows\voice-bundle.ps1 [-Out DIR] [-Python python]
# -Python: a CPython 3.13 x64 that runs pip (the wheels it picks must match the embedded 3.13).
# Needs Visual Studio (or its Build Tools) with the C++ tools, for its VC++ runtime: GitHub's
# windows-latest has it. voice-imports.py then checks every DLL's imports are in the bundle or Windows.
param(
    [string]$Out = (Join-Path $PSScriptRoot 'build\voice'),
    [string]$Python = 'python'
)
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')

# The pin: installer/manifest's python_embed and its SHA-256 (her pack's Python is the same one).
$manifest = Get-Content (Join-Path $repo 'installer\manifest')
$pyVer = (($manifest | Where-Object { $_ -like 'python_embed=*' }) -split '=', 2)[1]
$pySha = (($manifest | Where-Object { $_ -like 'python_embed_sha256=*' }) -split '=', 2)[1]
if (-not $pyVer -or -not $pySha) { throw 'installer/manifest has no python_embed or python_embed_sha256' }
$tag = 'python' + (($pyVer -split '\.')[0..1] -join '')

$v = & $Python -c "import sys, platform; print('%d.%d' % sys.version_info[:2], platform.machine())"
if ($LASTEXITCODE -or $v -ne '3.13 AMD64') { throw "-Python must be CPython 3.13 x64 (got: $v)" }

if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force $Out | Out-Null
$zip = Join-Path ([IO.Path]::GetTempPath()) "python-$pyVer-embed-amd64.zip"
if (-not (Test-Path $zip) -or (Get-FileHash -Algorithm SHA256 $zip).Hash -ne $pySha) {
    Invoke-WebRequest -UseBasicParsing "https://www.python.org/ftp/python/$pyVer/python-$pyVer-embed-amd64.zip" -OutFile $zip
}
$got = (Get-FileHash -Algorithm SHA256 $zip).Hash
if ($got -ne $pySha) { throw "python-$pyVer-embed-amd64.zip: SHA-256 $got, pinned $pySha" }
$py = Join-Path $Out 'python'
Expand-Archive -Path $zip -DestinationPath $py

# The Visual C++ runtime, app-local beside python.exe. The wheels' DLLs import it by name, and a clean
# Windows doesn't have it: onnxruntime's MSVCP140.dll and MSVCP140_1.dll, espeak-ng's MSVCP140.dll.
# Python loads extension modules (and ctypes DLLs by path) with LOAD_LIBRARY_SEARCH_DEFAULT_DIRS:
# python.exe's folder before System32, so these also win over an older copy there. Not vc_redist: both
# installers are per-user (PrivilegesRequired=lowest). From the newest x64\Microsoft.VC14x.CRT of the
# Visual Studio here, unmodified (Distributable Code; its terms: voice-licenses.py's VC_RUNTIME); the
# embeddable's vcruntime140*.dll stay when newer, so msvcp140 never runs on an older vcruntime140.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = if (Test-Path $vswhere) { & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath }
if (-not $vs) { throw "no Visual Studio with its C++ tools (vswhere: $vswhere): the bundle's VC++ runtime comes from it" }
$crt = Get-ChildItem -Directory (Join-Path $vs 'VC\Redist\MSVC') -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } | Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Get-ChildItem -Directory (Join-Path $_.FullName 'x64') -Filter 'Microsoft.VC14*.CRT' -ErrorAction SilentlyContinue } |
    Where-Object { Test-Path (Join-Path $_.FullName 'msvcp140_1.dll') } | Select-Object -First 1
if (-not $crt) { throw "no x64\Microsoft.VC14*.CRT with msvcp140_1.dll under $vs\VC\Redist\MSVC" }
foreach ($n in 'msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
    $src = Join-Path $crt.FullName $n
    $dst = Join-Path $py $n
    if (-not (Test-Path $src)) { throw "no $n in $($crt.FullName)" }
    if (-not (Test-Path $dst) -or (Get-Item $src).VersionInfo.FileVersionRaw -gt (Get-Item $dst).VersionInfo.FileVersionRaw) {
        Copy-Item -Force $src $dst
    }
}
Write-Output "VC++ runtime ($($crt.FullName)): $((Get-ChildItem $py -Filter '*140*.dll' | ForEach-Object { "$($_.Name) $($_.VersionInfo.FileVersionRaw)" }) -join ', ')"

# Its search path: the stdlib zip, its own dir, the wheels. (No `import site`: nothing here needs
# .pth files, and his user site-packages stays out.)
Set-Content -Encoding ascii -Path (Join-Path $py "$tag._pth") -Value @("$tag.zip", '.', '..\site-packages')

$site = Join-Path $Out 'site-packages'
& $Python -m pip install --disable-pip-version-check -q --no-compile --only-binary ':all:' --target $site `
    -r (Join-Path $repo 'tools\capri-voice-win-requirements.txt')
if ($LASTEXITCODE) { throw "pip failed ($LASTEXITCODE)" }
# What pip leaves that the sidecar never runs.
Get-ChildItem -Path $site -Directory -Filter 'bin' | Remove-Item -Recurse -Force
# sounddevice's wheel carries PortAudio for every platform, and builds of it with Steinberg's ASIO SDK
# (GPL-3.0 or Steinberg's own terms). She loads libportaudio64bit.dll only (never SD_ENABLE_ASIO):
# the rest goes. PortAudio's licence: voice-licenses.py (licenses\portaudio\).
$pa = Join-Path $site '_sounddevice_data\portaudio-binaries'
if (-not (Test-Path (Join-Path $pa 'libportaudio64bit.dll'))) { throw "no libportaudio64bit.dll in $pa (sounddevice's wheel changed)" }
Get-ChildItem $pa -Force | Where-Object Name -NotIn 'libportaudio64bit.dll', 'README.md' | Remove-Item -Recurse -Force

# Every DLL's imports in the bundle or Windows, the VC++ runtime from the bundle only and at least as
# new as the MSVC that linked what imports it (onnxruntime is unpinned: a version that imports one more
# runtime DLL, or was built by a newer MSVC than this Visual Studio's runtime, stops here, not on his PC).
& $Python (Join-Path $PSScriptRoot 'voice-imports.py') $Out
if ($LASTEXITCODE) { throw "voice-imports.py: the bundle imports DLLs a clean Windows wouldn't have ($LASTEXITCODE)" }

Copy-Item (Join-Path $repo 'tools\capri-voice') $Out
Copy-Item (Join-Path $repo 'tools\capri_voice_win.py') $Out
Copy-Item (Join-Path $PSScriptRoot 'voice-models.ps1') $Out
New-Item -ItemType Directory -Force (Join-Path $Out 'wake') | Out-Null
Copy-Item (Join-Path $repo 'tools\wake\hey-capri-v1.npz') (Join-Path $Out 'wake')
Copy-Item (Join-Path $repo 'tools\wake\NOTICE') (Join-Path $Out 'wake')
Copy-Item (Join-Path $repo 'LICENSE') $Out
& $Python (Join-Path $PSScriptRoot 'voice-licenses.py') $Out
if ($LASTEXITCODE) { throw "voice-licenses.py failed ($LASTEXITCODE)" }
& $Python -m pip freeze --path $site | Set-Content -Encoding utf8 (Join-Path $Out 'versions.txt')

$mb = [math]::Round(((Get-ChildItem -Recurse -File $Out | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Output "voice bundle: $Out ($mb MB, CPython $pyVer embeddable)"
