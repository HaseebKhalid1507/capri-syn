# Checks voice-models.ps1 on the small files (CI: ~5 MB from the net): a fetch, a cut download
# resumed from its .part, a corrupt one deleted with the script failing, a file already there kept.
#
#   .\packaging\windows\test-voice-models.ps1
$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'voice-models.ps1'
$to = Join-Path ([IO.Path]::GetTempPath()) "capri-voice-models-$PID"
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}
# True when the script ran through (its lines go to the host, so the answer is the only output).
function Fetch([string[]]$only) {
    try { & $script -To $to -Only $only | ForEach-Object { Write-Host "  $_" }; return $true }
    catch { Write-Host "  (threw: $($_.Exception.Message))"; return $false }
}
$small = 'silero_vad.onnx', 'melspectrogram.onnx', 'embedding_model.onnx', 'hey-capri-v1.npz'
try {
    Check (Fetch $small) 'the small models fetch'
    foreach ($n in $small) { Check (Test-Path (Join-Path $to $n)) "$n is there" }
    Check (-not (Get-ChildItem $to -Filter '*.part')) 'no .part left'

    # A cut download: half of it as the .part, the file gone. The run goes on from the half.
    $f = Join-Path $to 'silero_vad.onnx'
    $bytes = [IO.File]::ReadAllBytes($f)
    [IO.File]::WriteAllBytes("$f.part", $bytes[0..([int]($bytes.Length / 2) - 1)])
    Remove-Item $f
    $out = & $script -To $to -Only 'silero_vad.onnx'
    $out | ForEach-Object { Write-Host "  $_" }
    Check (($out -join "`n") -match 'resuming at') 'a cut download resumes'
    Check ((Test-Path $f) -and -not (Test-Path "$f.part")) 'and completes'

    # A corrupt one (right size, wrong bytes): deleted, and the script fails.
    Remove-Item $f
    [IO.File]::WriteAllBytes("$f.part", (New-Object byte[] $bytes.Length))
    Check (-not (Fetch @('silero_vad.onnx'))) 'a wrong hash fails the run'
    Check (-not (Test-Path "$f.part") -and -not (Test-Path $f)) 'and its file is deleted'
    Check (Fetch @('silero_vad.onnx')) 'the next run fetches it afresh'

    # Already there: kept, nothing fetched.
    $out = & $script -To $to -Only 'melspectrogram.onnx'
    Check (($out -join "`n") -match 'have it') 'a file with the right hash is kept'

    Check (-not (Fetch @('no-such-model'))) 'an unknown name is refused'
} finally {
    Remove-Item -Recurse -Force $to -ErrorAction SilentlyContinue
}
if ($fails.Count) { throw "$($fails.Count) model-fetch check(s) failed: $($fails -join '; ')" }
Write-Output 'all model-fetch checks passed'
