# install.ps1 end to end on Windows, as his own user: CI's windows-installer job runs it with the
# installer it just built; it also runs as is in the capri-win VM. A dry run (from GitHub when -Ref
# is given: as a scriptblock, and as `irm | iex` itself, which must leave his session as it was) that
# touches nothing; the install; what it installed works (synaps --version, her --check, the daemon);
# again (idempotent); --uninstall leaves nothing of hers (Synaps stays, as install.ps1 says). Then a
# --prefix with spaces over a pack he made himself (moved aside, never removed), and --purge (her
# files only).
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

    # `irm | iex` itself: it runs in the caller's scope, takes its flags from CAPRI_INSTALL_FLAGS,
    # and leaves his session as it was (preferences; none of its variables or functions).
    $env:CAPRI_INSTALL_FLAGS = '--dry-run --voice'
    $probe = "`$ErrorActionPreference = 'Continue'; `$ProgressPreference = 'Continue'; " +
        "irm 'https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/$Ref/install.ps1' | iex; " +
        "'iex-after: EAP=' + `$ErrorActionPreference + ' PP=' + `$ProgressPreference + " +
        "' vars=' + @(Get-Variable -Scope Global -Name Dry, Yes, Prefix, Prefixes, M, Tmp, Data, Steps, RepoDefault, Usage -ErrorAction SilentlyContinue).Count + " +
        "' fns=' + @(Get-Command Invoke-CapriInstall, Write-Info, Test-Removable -ErrorAction SilentlyContinue).Count + " +
        "' mods=' + @(Get-Module capri-install).Count + ' env=[' + `$env:CAPRI_INSTALL_FLAGS + ']'"
    $log = Join-Path $logs 'dry-run-iex.log'
    & $Shell -NoProfile -ExecutionPolicy Bypass -Command $probe *> $log
    $code = $LASTEXITCODE
    Remove-Item Env:\CAPRI_INSTALL_FLAGS
    Get-Content $log | ForEach-Object { Write-Host "    $_" }
    $text = Get-Content -Raw $log
    Check ($code -eq 0) "irm | iex (CAPRI_INSTALL_FLAGS=--dry-run --voice) exits 0 ($code)"
    Check ($text -match 'dry run: nothing was') 'it gets to the summary'
    Check ($text -match 'iex-after: EAP=Continue PP=Continue vars=0 fns=0 mods=0 env=\[\]') "his session is as it was, CAPRI_INSTALL_FLAGS cleared ($([regex]::Match($text, 'iex-after:[^\r\n]*').Value))"
    Check ($text -match 'flags from CAPRI_INSTALL_FLAGS: --dry-run --voice') 'the banner says where its flags came from'
    Check ($text -match [regex]::Escape("remove her: & ([scriptblock]::Create((irm https://raw.githubusercontent.com/HaseebKhalid1507/capri-syn/$Ref/install.ps1))) --uninstall")) 'piped: the remove-her line is the piped form'
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
# The end: she's started (or it says why not), and the sign-in is hers to walk him through (no picker).
$installText = Get-Content -Raw (Join-Path $logs 'install.log')
Check ($installText -match "she's up \(\d+ ms|she's still starting|she didn't start|no desktop session here") "it starts her at the end: $([regex]::Match($installText, "she's up[^\r\n]*|she's still starting|she didn't start[^\r\n]*|no desktop session here").Value)"
Check ($installText -match 'her brain: Synaps has no model yet') 'no model yet: it says she will walk him through it'
Check ($installText -notmatch 'pick one now') 'no sign-in picker in the installer'
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
Check ((Get-RunValue 'synaps-daemon') -match '^"[^"]*\\conhost\.exe" --headless "[^"]*synaps\.exe" daemon --detach$') 'the daemon starts at sign-in, hidden (Run\synaps-daemon: conhost --headless)'
Check (-not (Get-RunValue 'synaps-avatar')) 'no autostart for her (--no-autostart)'
$r = Get-Content $receipt
Check (($r | Where-Object { $_ -like "prefix`t*" }) -contains "prefix`t$app") 'the receipt records her prefix'
Check (-not ($r | Where-Object { $_ -eq "dir`t$app" })) 'and never as a folder to remove whole'

# 2. Again: idempotent.
$code = Invoke-Installer 'rerun' @('--yes', '--no-autostart', '--artifact', $Setup)
Check ($code -eq 0) "a rerun exits 0 ($code)"
$rerun = Get-Content -Raw (Join-Path $logs 'rerun.log')
Check ($rerun -match 'up to date \(') 'her pack is up to date (not rebuilt)'
Check ($rerun -match 'already set: Run\\synaps-daemon') 'the Run value is already set'
if ($installText -match "she's up \(") {
    # She runs (bare, this session): stopped by pid for her binary, said so, and started again.
    $stop = [regex]::Match($rerun, 'Stop-Process -Id \d+ \(her binary is replaced; pid \d+\)').Value
    Check ([bool]$stop) "a rerun stops the run going, by pid, and says so: $stop"
    Check ($rerun -match "she's up \(\d+ ms") 'and starts her again'
}

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
Check ((Get-Content -Raw (Join-Path $logs 'uninstall.log')) -notmatch 'left alone') 'every receipt entry was hers to remove'
$left = if (Test-Path $data) { @(Get-ChildItem -Recurse -Force $data | ForEach-Object { $_.FullName }) } else { @() }
Write-Output "left in ${data}: $(if ($left) { $left -join ', ' } else { 'nothing (the folder is gone)' })"

# 4. A --prefix with spaces, over a pack he made himself: his is moved aside, never replaced or removed.
$own = Join-Path $pack 'made-by-hand.txt'
New-Item -ItemType Directory -Force $pack | Out-Null
Set-Content -LiteralPath $own 'his own pack'
$spaced = Join-Path $env:LOCALAPPDATA 'Programs\My Apps\synaps-avatar'
# ...and his own Run\synaps-daemon: never overwritten, never recorded, still there after --uninstall.
$hisRun = '"C:\his\synaps.exe" daemon --detach --his'
New-ItemProperty -Path $runKey -Name 'synaps-daemon' -Value $hisRun -PropertyType String -Force | Out-Null
$code = Invoke-Installer 'install-spaced' @('--yes', '--no-autostart', '--artifact', $Setup, '--prefix', $spaced)
Check ($code -eq 0) "install --prefix '$spaced' exits 0 ($code)"
Check (Test-Path -LiteralPath (Join-Path $spaced 'synaps-avatar.exe')) "her exe in $spaced"
$aside = @(Get-ChildItem -Directory -Path (Split-Path -Parent $pack) -Filter 'capri.pre-install-*' | ForEach-Object { $_.FullName })
Check ($aside.Count -eq 1 -and (Test-Path -LiteralPath (Join-Path "$($aside[0])" 'made-by-hand.txt'))) "his pack went aside, whole ($($aside -join ', '))"
Check (Test-Path (Join-Path $pack 'sprites.json')) 'and hers is in its place'
Check (-not (Select-String -LiteralPath $receipt -SimpleMatch -Pattern 'pre-install' -Quiet)) 'the receipt has nothing of his pack'
$code = Invoke-Installer 'uninstall-spaced' @('--uninstall')
Check ($code -eq 0) "--uninstall exits 0 ($code)"
Check (-not (Test-Path -LiteralPath $spaced)) "$spaced is gone"
Check (-not (Test-Path -LiteralPath (Split-Path -Parent $spaced))) 'and My Apps (made for her)'
Check (-not (Test-Path $pack)) 'her pack is gone'
Check ($aside.Count -eq 1 -and (Test-Path -LiteralPath (Join-Path "$($aside[0])" 'made-by-hand.txt'))) 'his pack stays'
Check ((Get-Content -Raw (Join-Path $logs 'uninstall-spaced.log')) -notmatch 'left alone') 'every receipt entry was hers to remove'
Check ((Get-Content -Raw (Join-Path $logs 'install-spaced.log')) -match 'your own Run\\synaps-daemon') 'it said his Run\synaps-daemon is his'
Check ((Get-RunValue 'synaps-daemon') -eq $hisRun) "his own Run\synaps-daemon is as it was ($(Get-RunValue 'synaps-daemon'))"
Remove-ItemProperty -Path $runKey -Name 'synaps-daemon'

# 5. --purge: her settings, state and log; another pack, and his from before, stay.
$conf = Join-Path $env:APPDATA 'synaps-avatar'
$other = Join-Path $data 'packs\mine'
New-Item -ItemType Directory -Force $other, $conf | Out-Null
Set-Content (Join-Path $other 'pack.txt') 'his'
Set-Content (Join-Path $conf 'settings.toml') '[voice]'
Set-Content (Join-Path $data 'state.json') '{}'
$code = Invoke-Installer 'purge' @('--uninstall', '--purge')
Check ($code -eq 0) "--uninstall --purge exits 0 ($code)"
Check (-not (Test-Path (Join-Path $conf 'settings.toml'))) 'her settings are gone'
Check (-not (Test-Path (Join-Path $data 'state.json'))) 'her state is gone'
Check (-not (Test-Path (Join-Path $data 'synaps-avatar.log'))) 'her log is gone'
Check (Test-Path (Join-Path $other 'pack.txt')) 'another pack stays (packs\mine)'
Check ($aside.Count -eq 1 -and (Test-Path -LiteralPath "$($aside[0])")) 'his pack from before stays'
Remove-Item -Recurse -Force -LiteralPath $other
foreach ($a in $aside) { Remove-Item -Recurse -Force -LiteralPath $a }

& $synaps daemon stop *> $null
if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'all install.ps1 checks passed'
