# install.ps1 end to end on Windows, as his own user: CI's windows-installer job runs it with the
# installer it just built; it also runs as is in the capri-win VM. A dry run (from GitHub when -Ref
# is given, as `irm | iex` users get it) that touches nothing; the install; what it installed works
# (synaps --version, her --check, the daemon); again (idempotent); --uninstall leaves nothing of hers
# (Synaps stays, as install.ps1 says).
#
#   .\installer\test-windows.ps1 -Setup .\packaging\windows\Output\synaps-avatar-setup-....exe [-Ref SHA]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'The logs are echoed beside the checks; the functions return exit codes.')]
param(
    [Parameter(Mandatory)] [string]$Setup,
    [string]$Ref,
    # The host install.ps1 runs in: Windows PowerShell 5.1, what `irm | iex` gets on Windows 11.
    [string]$Shell = 'powershell'
)
$ErrorActionPreference = 'Stop'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}

$root = Split-Path -Parent $PSScriptRoot
$install = Join-Path $root 'install.ps1'
$Setup = (Resolve-Path $Setup).Path
$app = Join-Path $env:LOCALAPPDATA 'Programs\synaps-avatar'
$data = Join-Path $env:LOCALAPPDATA 'synaps-avatar'
$pack = Join-Path $data 'packs\capri'
$receipt = Join-Path $data 'install.receipt'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
# Where synaps' own installer (cargo-dist) puts it, as install.ps1 looks.
$synaps = Join-Path $(if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }) 'bin\synaps.exe'
$logs = Join-Path ([IO.Path]::GetTempPath()) "capri-install-test-$PID"
New-Item -ItemType Directory -Force $logs | Out-Null
if (Test-Path $app) { throw "$app exists already: not a clean machine" }

# Runs install.ps1 in $Shell with these flags; its output into a log (shown); the exit code.
function Invoke-Installer([string]$Name, [string[]]$Flags) {
    $log = Join-Path $logs "$Name.log"
    $t0 = Get-Date
    $ErrorActionPreference = 'Continue'
    & $Shell -NoProfile -ExecutionPolicy Bypass -File $install @Flags *> $log
    $code = $LASTEXITCODE
    Get-Content $log | ForEach-Object { Write-Host "    $_" }
    Write-Host "    (${Name}: exit $code, $([int]((Get-Date) - $t0).TotalSeconds) s)"
    return $code
}
function Get-RunValue([string]$Name) { (Get-ItemProperty -Path $runKey -Name $Name -ErrorAction SilentlyContinue).$Name }

# 0. A dry run touches nothing; from GitHub as `irm | iex` users get it, when -Ref is given.
$before = @(Get-ChildItem -Force $env:LOCALAPPDATA, $env:APPDATA -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
if ($Ref) {
    $env:CAPRI_INSTALL_REF = $Ref
    $remote = "& ([scriptblock]::Create((Invoke-RestMethod 'https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/$Ref/install.ps1'))) --dry-run --voice"
    $log = Join-Path $logs 'dry-run-remote.log'
    & $Shell -NoProfile -ExecutionPolicy Bypass -Command $remote *> $log
    $code = $LASTEXITCODE
    Get-Content $log | ForEach-Object { Write-Host "    $_" }
    Check ($code -eq 0) "the remote dry run (scriptblock, --dry-run --voice) exits 0 ($code)"
    Check ((Get-Content -Raw $log) -match 'dry run: nothing was') 'it gets to the summary'
}
$code = Invoke-Installer 'dry-run' @('--dry-run', '--artifact', $Setup)
Check ($code -eq 0) "--dry-run exits 0 ($code)"
$after = @(Get-ChildItem -Force $env:LOCALAPPDATA, $env:APPDATA -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
$new = @(Compare-Object $before $after -PassThru | Where-Object { $_ -notmatch 'Temp|capri-install-test' })
Check ($new.Count -eq 0) "--dry-run made nothing in %LOCALAPPDATA% or %APPDATA% ($($new -join ', '))"
Check (-not (Test-Path $app)) '--dry-run installed nothing'

# 1. The install: her installer (the one CI built), unattended, with no autostart.
$code = Invoke-Installer 'install' @('--yes', '--no-autostart', '--artifact', $Setup)
Check ($code -eq 0) "install.ps1 --yes --no-autostart --artifact exits 0 ($code)"
Check (Test-Path $synaps) "synaps in $synaps"
$v = if (Test-Path $synaps) { (& $synaps --version) | Select-Object -First 1 } else { '' }
Check ($v -match '^synaps \d+\.\d+\.\d+') "synaps --version: $v"
Check (Test-Path (Join-Path $app 'synaps-avatar.exe')) "her exe in $app"
Check (Test-Path (Join-Path $pack 'sprites.json')) "her pack in $pack"
Check (Test-Path (Join-Path $pack 'persona.md')) 'with her persona'
Check (Test-Path (Join-Path $pack 'anim\lachy\LICENSE.md')) "and lachy's art, with its licence"
Check (Test-Path $receipt) "a receipt ($receipt)"
# Her --check: the windowed exe writes to her log.
$herLog = Join-Path $data 'synaps-avatar.log'
$p = Start-Process -FilePath (Join-Path $app 'synaps-avatar.exe') -ArgumentList '--check' -Wait -PassThru -WindowStyle Hidden
$text = if (Test-Path $herLog) { Get-Content -Raw $herLog } else { '' }
Check ($p.ExitCode -eq 0) "synaps-avatar --check exits 0 ($($p.ExitCode))"
Check ($text -match 'pack `Capri` ok: \d+ frames') "her log: $([regex]::Match($text, 'pack `Capri` ok: \d+ frames[^,]*').Value)"
$ErrorActionPreference = 'Continue'
$status = (& $synaps daemon status 2>&1 | Out-String)
$ErrorActionPreference = 'Stop'
Check ($status -match 'daemon: running') 'the Synaps daemon runs'
Check ($status -match 'protocol: 3') 'protocol 3 (her client)'
Check ((Get-RunValue 'synaps-daemon') -match 'synaps\.exe" daemon --detach$') 'the daemon starts at sign-in (Run\synaps-daemon)'
Check (-not (Get-RunValue 'synaps-avatar')) 'no autostart for her (--no-autostart)'

# 2. Again: idempotent.
$code = Invoke-Installer 'rerun' @('--yes', '--no-autostart', '--artifact', $Setup)
Check ($code -eq 0) "a rerun exits 0 ($code)"
$rerun = Get-Content -Raw (Join-Path $logs 'rerun.log')
Check ($rerun -match 'up to date \(') 'her pack is up to date (not rebuilt)'
Check ($rerun -match 'already set: Run\\synaps-daemon') 'the Run value is already set'

# 3. --uninstall: nothing of hers is left; Synaps stays.
$code = Invoke-Installer 'uninstall' @('--uninstall')
Check ($code -eq 0) "--uninstall exits 0 ($code)"
Check (-not (Test-Path $app)) "$app is gone"
Check (-not (Test-Path $pack)) 'her pack is gone'
Check (-not (Test-Path $receipt)) 'the receipt is gone'
Check (-not (Test-Path (Join-Path $data 'installer'))) "the installer's cache is gone"
Check (-not (Get-RunValue 'synaps-daemon')) 'Run\synaps-daemon is gone'
Check (-not (Test-Path 'HKCU:\Software\Classes\AppUserModelId\Synaps.Avatar')) 'her AUMID is gone'
Check (-not (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{2147942F-E90C-4536-BEA2-8AB4B7F51734}_is1')) 'her Apps entry is gone'
Check (Test-Path $synaps) 'synaps stays'
$left = if (Test-Path $data) { @(Get-ChildItem -Recurse -Force $data | ForEach-Object { $_.FullName }) } else { @() }
Write-Output "left in ${data}: $(if ($left) { $left -join ', ' } else { 'nothing (the folder is gone)' })"

& $synaps daemon stop *> $null
if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'all install.ps1 checks passed'
