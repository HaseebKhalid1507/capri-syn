# Installs, checks and uninstalls her VOICE installer, unattended, as his own user (CI runs it after
# build-voice.ps1; it also runs as is in the capri-win VM). The models task is left out (~320 MB;
# voice-models.ps1 has its own test): the bundle's own interpreter runs the selftest without them.
#
#   .\packaging\windows\test-voice-install.ps1 -Setup .\packaging\windows\Output\capri-syn-voice-setup-….exe
param([Parameter(Mandatory)] [string]$Setup)
$ErrorActionPreference = 'Stop'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}
function RunFor([string]$exe, [string[]]$argv, [int]$secs = 300) {
    $p = Start-Process $exe -ArgumentList $argv -PassThru
    $null = $p.Handle
    if (-not $p.WaitForExit($secs * 1000)) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Write-Host "FAIL $(Split-Path $exe -Leaf) still running after $secs s: killed"
        return -1
    }
    return $p.ExitCode
}
function Install {
    $log = Join-Path ([IO.Path]::GetTempPath()) "sa-voice-setup-$PID.log"
    $code = RunFor $Setup @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/MERGETASKS="!models"', "/LOG=`"$log`"")
    if (Test-Path $log) { Get-Content $log -Tail 8 | ForEach-Object { Write-Host "  setup: $_" } }
    return $code
}
function Uninstall([string]$voice) {
    $code = RunFor (Join-Path $voice 'unins000.exe') @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
    # The uninstaller re-runs itself from %TEMP%: wait for it to finish.
    $deadline = (Get-Date).AddSeconds(120)
    while ((Test-Path (Join-Path $voice 'unins000.exe')) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    Start-Sleep -Seconds 2
    return $code
}

$state = Join-Path $env:LOCALAPPDATA 'capri-voice'
$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\synaps-avatar voice models.lnk'
$her = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{2147942F-E90C-4536-BEA2-8AB4B7F51734}_is1'
$voiceKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7E0CA8F-064C-4B7F-939A-41681146411C}_is1'

# 1. She isn't installed: voice goes where her installer would put her, + \voice (she looks in
#    "voice" beside her exe).
$voice = Join-Path $env:LOCALAPPDATA 'Programs\synaps-avatar\voice'
if (Test-Path $voice) { throw "$voice exists already: not a clean machine" }
$code = Install
Check ($code -eq 0) "installs unattended, without the models (exit $code)"
foreach ($f in 'python\python.exe', 'python\python313._pth', 'capri-voice', 'capri_voice_win.py', 'voice-models.ps1', 'wake\hey-capri-v1.npz',
               'site-packages\onnxruntime', 'site-packages\kokoro_onnx', 'unins000.exe') {
    Check (Test-Path (Join-Path $voice $f)) "$f in $voice"
}
Check (Test-Path $lnk) 'a Start-menu shortcut to fetch the models again'
Check (-not (Test-Path (Join-Path $state 'models'))) 'no models fetched (the task was left out)'
Check ((Get-ItemProperty $voiceKey -ErrorAction SilentlyContinue).DisplayName -eq 'capri-syn voice') 'its own entry in Apps (capri-syn voice)'

# The bundle runs from where it was installed, with no Python on PATH needed.
$env:CAPRI_VOICE_MODELS = Join-Path ([IO.Path]::GetTempPath()) "sa-no-models-$PID"
$out = & (Join-Path $voice 'python\python.exe') (Join-Path $voice 'capri-voice') --selftest
$sc = $LASTEXITCODE
Remove-Item Env:\CAPRI_VOICE_MODELS
$out | Select-Object -Last 3 | ForEach-Object { Write-Host "  selftest: $_" }
Check ($sc -eq 0) "the installed bundle runs the sidecar's selftest (exit $sc)"

# What runs leave (its log, a link, Python's caches) goes at uninstall too.
New-Item -ItemType Directory -Force (Join-Path $state 'models') | Out-Null
Set-Content (Join-Path $state 'capri-voice.log') 'a run'
Set-Content (Join-Path $state 'models\stand-in.bin') 'not a model'
$code = Uninstall $voice
Check ($code -eq 0) "uninstalls unattended (exit $code)"
Check (-not (Test-Path $voice)) "nothing left in $voice"
Check (-not (Test-Path $state)) "nothing left in $state (models, log)"
Check (-not (Test-Path $lnk)) 'the shortcut is gone'
Check (-not (Test-Path $voiceKey)) 'its Apps entry is gone'

# 2. She is installed somewhere else: voice goes beside her there.
$elsewhere = Join-Path ([IO.Path]::GetTempPath()) "sa-her-$PID"
New-Item -ItemType Directory -Force $elsewhere | Out-Null
New-Item -Force $her | Out-Null
Set-ItemProperty $her -Name InstallLocation -Value "$elsewhere\"
try {
    $code = Install
    Check ($code -eq 0 -and (Test-Path (Join-Path $elsewhere 'voice\python\python.exe'))) "beside her where she is ($elsewhere\voice)"
    $code = Uninstall (Join-Path $elsewhere 'voice')
    Check (-not (Test-Path (Join-Path $elsewhere 'voice'))) 'and gone from there at uninstall'
} finally {
    Remove-Item -Force $her -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $elsewhere -ErrorAction SilentlyContinue
}

if ($fails.Count) { throw "$($fails.Count) voice installer check(s) failed: $($fails -join '; ')" }
Write-Output 'all voice installer checks passed'
