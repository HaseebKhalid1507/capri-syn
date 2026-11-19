# Her installer with her voice, unattended, on a clean Windows (CI's windows-setup-voice job): the
# setup with /VOICE=on (a CI runner has no microphone, so the page's default would be off), with the
# voice installer CI built beside it (the setup takes one it finds there; a release's takes the
# release's). Then:
#   - the voice bundle beside her, its selftest run by its own interpreter, the models in place;
#   - settings.toml: [voice] enabled = true (and "hey Capri", replies aloud) through her own writer,
#     and never her voice itself (voice, speed, pace, flow stay hers);
#   - she starts, and her log says the sidecar started;
#   - the uninstall leaves no voice: the bundle, %LOCALAPPDATA%\capri-voice, its Apps entry.
#
# Until her binary has `--set` (#84, on #77), the checks that need her choices saved (settings.toml,
# the sidecar started) say SKIP with the reason, and the rest still run.
#
#   pwsh .\packaging\windows\test-setup-voice.ps1 -Setup DIR\capri-syn-setup-....exe   (the voice installer beside it)
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
param([Parameter(Mandatory)] [string]$Setup)
$ErrorActionPreference = 'Stop'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}
$Setup = (Resolve-Path $Setup).Path
if (-not (Get-ChildItem (Split-Path -Parent $Setup) -Filter 'capri-syn-voice-setup*.exe')) { throw "no capri-syn-voice-setup*.exe beside $Setup" }
$app = Join-Path $env:LOCALAPPDATA 'Programs\synaps-avatar'
$exe = Join-Path $app 'synaps-avatar.exe'
$voice = Join-Path $app 'voice'
$cv = Join-Path $env:LOCALAPPDATA 'capri-voice'
$models = Join-Path $cv 'models'
$herLog = Join-Path $env:LOCALAPPDATA 'synaps-avatar\synaps-avatar.log'
$settings = Join-Path $env:APPDATA 'synaps-avatar\settings.toml'
$voiceKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7E0CA8F-064C-4B7F-939A-41681146411C}_is1'
$inno = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{2147942F-E90C-4536-BEA2-8AB4B7F51734}_is1'
if (Test-Path $app) { throw "$app exists already: not a clean machine" }

# 1. Her installer, unattended, with her voice.
$log = Join-Path ([IO.Path]::GetTempPath()) "capri-setup-voice-$PID.log"
$t0 = Get-Date
$p = Start-Process $Setup -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/VOICE=on', "/LOG=`"$log`"") -PassThru
$null = $p.Handle
if (-not $p.WaitForExit(30 * 60 * 1000)) { Stop-Process -Id $p.Id -Force; throw 'her installer still runs after 30 minutes' }
$setupLog = if (Test-Path $log) { Get-Content -Raw $log } else { '' }
($setupLog -split "`r?`n" | Where-Object { $_ -match '(step |voice|mics|finished): ' }) | ForEach-Object { Write-Host "  setup: $_" }
Check ($p.ExitCode -eq 0) "her installer with /VOICE=on exits 0 ($($p.ExitCode), $([int]((Get-Date) - $t0).TotalSeconds) s)"
Check ($setupLog -match 'voice: voice: installed ') 'the voice step installed her voice'
Check ($setupLog -match 'voicefrom: file [^\r\n]*capri-syn-voice-setup') "from the voice installer beside the setup: $([regex]::Match($setupLog, 'voicefrom: [^\r\n]*').Value)"
# Her binary without --set (before #84): the voice is installed, his choices can't be saved yet.
$noSet = $setupLog -match "can't save settings from her installer yet"
if ($noSet) { Write-Output "SKIP her choices saved, settings.toml, the sidecar started: this build of her has no --set yet (#84, on #77)" }
else { Check ($setupLog -match 'voice: voice: ok') "and saved his choices through her writer$(if ($setupLog -match 'voice: failed: [^\r\n]*') { ': ' + $Matches[0] })" }

# 2. The bundle, its selftest, the models.
$py = Join-Path $voice 'python\python.exe'
Check (Test-Path $py) "the voice bundle beside her ($voice)"
if (Test-Path $py) {
    Push-Location $voice
    try { $out = (& $py (Join-Path $voice 'capri-voice') --selftest 2>&1 | Out-String); $rc = $LASTEXITCODE } finally { Pop-Location }
    Write-Host ($out.Trim() -split "`n" | Select-Object -Last 5 | ForEach-Object { "  selftest: $_" } | Out-String)
    Check ($rc -eq 0) "its selftest, by its own interpreter (exit $rc)"
}
foreach ($m in 'kokoro-v1.0.int8.onnx', 'voices-v1.0.bin', 'ggml-small.en-q5_1.bin', 'silero_vad.onnx', 'hey-capri-v1.npz') {
    Check (Test-Path (Join-Path $models $m)) "model $m"
}

# 3. settings.toml: his choices, and nothing of her voice itself.
$text = if (Test-Path $settings) { Get-Content -Raw $settings } else { '' }
Write-Output "--- settings.toml ---`n$text---"
Check ($text -notmatch '(?m)^\s*(voice|speed|pace|flow)\s*=') 'never her voice itself (voice, speed, pace, flow: her default)'
if (-not $noSet) {
    Check ($text -match '(?m)^\s*enabled\s*=\s*true') '[voice] enabled = true'
    Check ($text -match '(?m)^\s*wake_word\s*=\s*true') '[voice] wake_word = true ("hey Capri", the default)'
    Check ($text -match '(?m)^\s*speak_unprompted\s*=\s*true') '[voice] speak_unprompted = true (replies aloud, the default)'
}

# 4. She starts her voice.
$from = if (Test-Path $herLog) { (Get-Item $herLog).Length } else { 0 }
$her = Start-Process $exe -WorkingDirectory $env:USERPROFILE -PassThru
$t0 = Get-Date
$said = ''
while (((Get-Date) - $t0).TotalSeconds -lt 120 -and -not $her.HasExited) {
    if (Test-Path $herLog) {
        $all = Get-Content -Raw $herLog
        $said = if ($all.Length -ge $from) { $all.Substring([int]$from) } else { $all }
        if ($said -match 'voice: sidecar started') { break }
    }
    Start-Sleep -Milliseconds 500
}
if ($noSet) { Write-Output "SKIP the sidecar started: voice isn't on without --set (her log: $([regex]::Match($said, 'voice: [^\r\n]*').Value))" }
else { Check ($said -match 'voice: sidecar started \(pid \d+') "her log: $([regex]::Match($said, 'voice: sidecar started[^\r\n]*').Value)" }
$vlog = Join-Path $cv 'capri-voice.log'
if (Test-Path $vlog) { Write-Output "--- capri-voice.log (end) ---`n$((Get-Content $vlog -Tail 15) -join "`n")`n---" }
Write-Output "--- her log (voice) ---`n$(($said -split "`r?`n" | Where-Object { $_ -match 'voice' } | Select-Object -Last 15) -join "`n")`n---"
Stop-Process -Id $her.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# 5. The uninstall: no voice is left.
$u = Start-Process (Join-Path $app 'unins000.exe') -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -PassThru
$null = $u.Handle
[void]$u.WaitForExit(300000)
$deadline = (Get-Date).AddSeconds(180)
while (((Test-Path $inno) -or (Test-Path $voiceKey)) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Start-Sleep -Seconds 2
Check ($u.ExitCode -eq 0) "uninstalls unattended (exit $($u.ExitCode))"
Check (-not (Test-Path $voice)) "the voice bundle is gone ($voice)"
Check (-not (Test-Path $cv)) "$cv is gone (models, log, link)$(if (Test-Path $cv) { ': ' + ((Get-ChildItem -Force $cv | ForEach-Object { $_.Name }) -join ', ') })"
Check (-not (Test-Path $voiceKey)) 'its Apps entry (capri-syn voice) is gone'
Check (-not (Test-Path $app)) "$app is gone"

if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'her installer with her voice works'
