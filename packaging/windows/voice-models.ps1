# Fetches capri-voice's models on Windows: the files and URLs tools/capri-voice-setup fetches on
# Linux, each SHA-256 pinned, into %LOCALAPPDATA%\capri-voice\models ($env:CAPRI_VOICE_MODELS, or
# -To DIR), where the sidecar reads them. No admin. Nothing of it is in the repo or the installers.
#
# Resumable: a cut download stays as <file>.part and goes on from where it stopped (curl.exe -C -,
# part of Windows since 10 1803). A file whose hash doesn't match its pin is deleted and the script
# fails (run it again for a fresh copy). A file already there with the right hash is kept.
#
#   .\voice-models.ps1                         # all of them (~320 MB)
#   .\voice-models.ps1 -Stt base.en            # Whisper base.en only (60 MB instead of 250)
#   .\voice-models.ps1 -Only silero_vad.onnx   # just these (tests)
#   .\voice-models.ps1 -To D:\models
#
# The wake word's two feature models are openWakeWord's (CC BY-NC-SA 4.0: personal use, as on
# Linux); "hey capri" (hey-capri-v1.npz) comes with the bundle (tools/wake), not from the net.
param(
    [string]$To,
    [string[]]$Only,
    [ValidateSet('small.en', 'base.en', 'both')] [string]$Stt = 'both'
)
$ErrorActionPreference = 'Stop'

# Whisper at a fixed revision of ggerganov/whisper.cpp (tools/capri-voice-setup pins the same).
$hf = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1'
$kokoro = 'https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0'
$oww = 'https://github.com/dscripka/openWakeWord/releases/download/v0.5.1'
# name, url, SHA-256, bytes. Whisper's hashes are Hugging Face's own (its LFS ids); Kokoro's
# release publishes none: these are of the files capri-voice-setup fetched from it (sizes match the
# release); Silero's and the wake word's are capri-voice-setup's pins.
$models = @(
    @('ggml-small.en-q5_1.bin', "$hf/ggml-small.en-q5_1.bin", 'bfdff4894dcb76bbf647d56263ea2a96645423f1669176f4844a1bf8e478ad30', 190098681),
    @('ggml-base.en-q5_1.bin', "$hf/ggml-base.en-q5_1.bin", '4baf70dd0d7c4247ba2b81fafd9c01005ac77c2f9ef064e00dcf195d0e2fdd2f', 59721011),
    @('kokoro-v1.0.int8.onnx', "$kokoro/kokoro-v1.0.int8.onnx", '6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb', 92361271),
    @('voices-v1.0.bin', "$kokoro/voices-v1.0.bin", 'bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d', 28214398),
    @('silero_vad.onnx', 'https://raw.githubusercontent.com/snakers4/silero-vad/v5.1.2/src/silero_vad/data/silero_vad.onnx', '2623a2953f6ff3d2c1e61740c6cdb7168133479b267dfef114a4a3cc5bdd788f', 2327524),
    @('melspectrogram.onnx', "$oww/melspectrogram.onnx", 'ba2b0e0f8b7b875369a2c89cb13360ff53bac436f2895cced9f479fa65eb176f', 1087958),
    @('embedding_model.onnx', "$oww/embedding_model.onnx", '70d164290c1d095d1d4ee149bc5e00543250a7316b59f31d056cff7bd3075c1f', 1326578),
    @('hey-capri-v1.npz', $null, '68ccb1ca224258e7be4383df7b8f6a6e11d4051ce13085aca9ecc0e63f1c43ab', 412514)
)
if ($Stt -eq 'small.en') { $models = @($models | Where-Object { $_[0] -ne 'ggml-base.en-q5_1.bin' }) }
if ($Stt -eq 'base.en') { $models = @($models | Where-Object { $_[0] -ne 'ggml-small.en-q5_1.bin' }) }
if ($Only) {
    $want = @($Only | ForEach-Object { $_ -split ',' } | Where-Object { $_ })
    $unknown = @($want | Where-Object { $n = $_; -not ($models | Where-Object { $_[0] -eq $n }) })
    if ($unknown.Count) { throw "unknown model(s): $($unknown -join ', ')" }
    $models = @($models | Where-Object { $want -contains $_[0] })
}

if (-not $To) { $To = $env:CAPRI_VOICE_MODELS }
if (-not $To) { $To = Join-Path $env:LOCALAPPDATA 'capri-voice\models' }
New-Item -ItemType Directory -Force $To | Out-Null
$curl = Join-Path $env:SystemRoot 'System32\curl.exe'
if (-not (Test-Path $curl)) { $curl = 'curl.exe' }

function Sha([string]$path) { (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant() }

# "hey capri": beside this script in the bundle (wake\), or in the repo (tools\wake).
$npz = @((Join-Path $PSScriptRoot 'wake\hey-capri-v1.npz'), (Join-Path $PSScriptRoot '..\..\tools\wake\hey-capri-v1.npz')) |
    Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1

$failed = @()
foreach ($m in $models) {
    $name, $url, $sha, $size = $m
    $dest = Join-Path $To $name
    $part = "$dest.part"
    if ((Test-Path -LiteralPath $dest) -and (Sha $dest) -eq $sha) {
        Write-Output "ok   $name (have it)"
        continue
    }
    if (-not $url) {
        if (-not $npz) { $failed += "$name (not beside this script)"; Write-Output "FAIL ${name}: not beside this script"; continue }
        Copy-Item -LiteralPath $npz -Destination $part -Force
    } else {
        $have = 0
        if (Test-Path -LiteralPath $part) { $have = (Get-Item -LiteralPath $part).Length }
        if ($have -gt $size) { Remove-Item -LiteralPath $part; $have = 0 }
        if ($have -lt $size) {
            $from = if ($have) { " (resuming at $([math]::Round($have / 1MB, 1)) MB)" } else { '' }
            Write-Output "get  $name, $([math]::Round($size / 1MB, 1)) MB$from"
            & $curl -fsSL --retry 3 --retry-delay 2 -C - -o $part $url
            if ($LASTEXITCODE) {
                # Kept: the next run goes on from here.
                $failed += "$name (curl $LASTEXITCODE)"
                Write-Output "FAIL ${name}: curl exit $LASTEXITCODE (the part is kept; run again to resume)"
                continue
            }
        }
    }
    $got = Sha $part
    if ($got -ne $sha) {
        Remove-Item -LiteralPath $part -Force
        $failed += "$name (SHA-256 $got, pinned $sha)"
        Write-Output "FAIL ${name}: SHA-256 $got is not the pinned $sha (deleted)"
        continue
    }
    Move-Item -LiteralPath $part -Destination $dest -Force
    Write-Output "ok   $name"
}
if ($failed.Count) { throw "models not fetched: $($failed -join '; ')" }
Write-Output "models: $To"
