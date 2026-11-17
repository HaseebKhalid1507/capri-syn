# Builds capri-voice's Windows bundle: a folder that runs the sidecar with no Python installed.
# Not in her installer yet (docs/design/windows-voice.md, "Packaging"); CI builds it and runs the
# selftest and the unit tests with the bundled interpreter.
#
#   <Out>\python\          CPython's embeddable package (python.org; pinned, SHA-256 checked), its
#                          python313._pth pointing at ..\site-packages
#   <Out>\site-packages\   tools\capri-voice-win-requirements.txt, wheels only (nothing compiled)
#   <Out>\capri-voice, <Out>\capri_voice_win.py
#   <Out>\voice-models.ps1, <Out>\wake\hey-capri-v1.npz   (the model fetch, and the one model in the repo)
#
# Run it:  <Out>\python\pythonw.exe <Out>\capri-voice      (no console; python.exe for one)
# Models are never in it: %LOCALAPPDATA%\capri-voice\models, fetched by voice-models.ps1 (what
# capri-voice-setup fetches on Linux). The voice installer (synaps-avatar-voice.iss) carries it.
#
#   .\packaging\windows\voice-bundle.ps1 [-Out DIR] [-Python python]
# -Python: a CPython 3.13 x64 that runs pip (the wheels it picks must match the embedded 3.13).
param(
    [string]$Out = (Join-Path $PSScriptRoot 'build\voice'),
    [string]$Python = 'python'
)
$ErrorActionPreference = 'Stop'
$repo = Resolve-Path (Join-Path $PSScriptRoot '..\..')

# The pin: the SHA-256 was taken from python.org's file when pinned (python.org publishes Sigstore
# bundles, not hashes); bump both together.
$pyVer = '3.13.17'
$pySha = '9e35319bffedab8b36ad44d8f377cb4f1733bcec257a0b6d04d94f01486ddb38'
$tag = 'python313'

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

# Its search path: the stdlib zip, its own dir, the wheels. (No `import site`: nothing here needs
# .pth files, and his user site-packages stays out.)
Set-Content -Encoding ascii -Path (Join-Path $py "$tag._pth") -Value @("$tag.zip", '.', '..\site-packages')

$site = Join-Path $Out 'site-packages'
& $Python -m pip install --disable-pip-version-check -q --no-compile --only-binary ':all:' --target $site `
    -r (Join-Path $repo 'tools\capri-voice-win-requirements.txt')
if ($LASTEXITCODE) { throw "pip failed ($LASTEXITCODE)" }
# What pip leaves that the sidecar never runs.
Get-ChildItem -Path $site -Directory -Filter 'bin' | Remove-Item -Recurse -Force

Copy-Item (Join-Path $repo 'tools\capri-voice') $Out
Copy-Item (Join-Path $repo 'tools\capri_voice_win.py') $Out
Copy-Item (Join-Path $PSScriptRoot 'voice-models.ps1') $Out
New-Item -ItemType Directory -Force (Join-Path $Out 'wake') | Out-Null
Copy-Item (Join-Path $repo 'tools\wake\hey-capri-v1.npz') (Join-Path $Out 'wake')
& $Python -m pip freeze --path $site | Set-Content -Encoding utf8 (Join-Path $Out 'versions.txt')

$mb = [math]::Round(((Get-ChildItem -Recurse -File $Out | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Output "voice bundle: $Out ($mb MB, CPython $pyVer embeddable)"
