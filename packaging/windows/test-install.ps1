# Installs, checks and uninstalls her installer, unattended, as his own user (CI runs it after
# build.ps1; it also runs as is in the capri-win VM). No art: a stand-in pack (a sprites.json she
# rejects) proves the copy and the default path without shipping hers.
#
#   .\packaging\windows\test-install.ps1 -Setup .\packaging\windows\Output\synaps-avatar-setup-….exe
param([Parameter(Mandatory)] [string]$Setup)
$ErrorActionPreference = 'Stop'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}

$app = Join-Path $env:LOCALAPPDATA 'Programs\synaps-avatar'
$data = Join-Path $env:LOCALAPPDATA 'synaps-avatar'
$packTo = Join-Path $data 'packs\capri'
$lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\synaps-avatar.lnk'
$aumid = 'HKCU:\Software\Classes\AppUserModelId\Synaps.Avatar'
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if (Test-Path $app) { throw "$app exists already: not a clean machine" }

$pack = Join-Path ([IO.Path]::GetTempPath()) "sa-stand-in-pack-$PID"
New-Item -ItemType Directory -Force (Join-Path $pack 'poses') | Out-Null
Set-Content (Join-Path $pack 'sprites.json') '{}'
Set-Content (Join-Path $pack 'poses\stand-in.txt') 'not art'
$notPack = Join-Path ([IO.Path]::GetTempPath()) "sa-not-a-pack-$PID"
New-Item -ItemType Directory -Force $notPack | Out-Null

# Runs `exe` and waits at most `secs`: a stuck one (a dialog nobody sees) fails instead of hanging.
function RunFor([string]$exe, [string[]]$argv, [int]$secs = 180) {
    $p = Start-Process $exe -ArgumentList $argv -PassThru
    $null = $p.Handle   # kept now, or ExitCode is lost once it exits
    if (-not $p.WaitForExit($secs * 1000)) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Write-Host "FAIL $(Split-Path $exe -Leaf) still running after $secs s: killed"
        return -1
    }
    return $p.ExitCode
}

function Install([string[]]$more) {
    $log = Join-Path ([IO.Path]::GetTempPath()) "sa-setup-$PID.log"
    $code = RunFor $Setup (@('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$log`"") + $more)
    if (Test-Path $log) { Get-Content $log -Tail 15 | ForEach-Object { Write-Host "  setup: $_" } }
    return $code
}

# A folder that isn't a pack is refused, and nothing is installed.
$code = Install @("/PACK=`"$notPack`"")
Check ($code -ne 0 -and -not (Test-Path (Join-Path $app 'synaps-avatar.exe'))) "a folder without sprites.json is refused (exit $code)"

$code = Install @("/PACK=`"$pack`"", '/TASKS=autostart')
Check ($code -eq 0) "installs unattended (exit $code)"
Check (Test-Path (Join-Path $app 'synaps-avatar.exe')) "her exe in $app"
Check (Test-Path (Join-Path $app 'unins000.exe')) 'an uninstaller'
Check (Test-Path (Join-Path $packTo 'sprites.json')) "the pack copied to $packTo"
Check (Test-Path (Join-Path $packTo 'poses\stand-in.txt')) "with its folders"
Check ((Get-ItemProperty $aumid -ErrorAction SilentlyContinue).DisplayName -eq 'synaps-avatar') 'her AUMID registered (DisplayName)'
Check ((Get-ItemProperty $aumid -ErrorAction SilentlyContinue).IconUri -eq (Join-Path $app 'synaps-avatar.png')) 'with her icon (IconUri)'
Check (Test-Path (Join-Path $app 'synaps-avatar.png')) 'the icon file'
Check ((Get-ItemProperty $run -ErrorAction SilentlyContinue).'synaps-avatar' -eq "`"$(Join-Path $app 'synaps-avatar.exe')`"") 'autostart (the Run key), when asked'
Check (Test-Path $lnk) 'a Start-menu shortcut'
# Settings > Apps shows EstimatedSize (KB). Sizing the pack before his pick once walked all of C:.
$size = (Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -eq 'synaps-avatar' } | Select-Object -First 1).EstimatedSize
Check ($size -gt 0 -and $size -lt 512000) "her size in Settings > Apps is hers ($size KB, under 500 MB)"
$item = (New-Object -ComObject Shell.Application).Namespace((Split-Path $lnk)).ParseName((Split-Path $lnk -Leaf))
Check ($item -and $item.ExtendedProperty('System.AppUserModel.ID') -eq 'Synaps.Avatar') "the shortcut carries her AUMID ($(if ($item) { $item.ExtendedProperty('System.AppUserModel.ID') }))"

# Her exe: windowed (no console), so she logs to a file; the pack she looks for by default is the
# one the installer put there (she rejects the stand-in, naming its path).
$exe = Join-Path $app 'synaps-avatar.exe'
$pe = [IO.File]::ReadAllBytes($exe)
Check ([BitConverter]::ToUInt16($pe, [BitConverter]::ToInt32($pe, 0x3C) + 0x5C) -eq 2) 'her exe is windowed (subsystem 2)'
$code = RunFor $exe @('--check') 60
$log = Join-Path $data 'synaps-avatar.log'
$text = if (Test-Path $log) { Get-Content -Raw $log } else { '' }
Write-Output "--- her log ---`n$text---"
Check ($code -eq 1) "--check on the stand-in pack fails (exit $code)"
Check ($text -match 'no console, logging here') 'without a console she logs to %LOCALAPPDATA%\synaps-avatar\synaps-avatar.log'
# She joins `synaps-avatar/packs/capri` onto %LOCALAPPDATA% as is.
Check ($text.Contains("$env:LOCALAPPDATA\synaps-avatar/packs/capri") -or $text.Contains("$packTo")) 'her default pack is the installed one'

# Uninstall: everything the installer and she made goes; nothing of hers is left running.
$ucode = RunFor (Join-Path $app 'unins000.exe') @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
# The uninstaller re-runs itself from %TEMP%: wait for it to finish.
$deadline = (Get-Date).AddSeconds(90)
while ((Test-Path (Join-Path $app 'unins000.exe')) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Start-Sleep -Seconds 2
Check ($ucode -eq 0) "uninstalls unattended (exit $ucode)"
Check (-not (Test-Path $app)) "$app is gone"
Check (-not (Test-Path $data)) "$data is gone (pack, log, state)"
Check (-not (Test-Path $aumid)) 'her AUMID key is gone'
Check ($null -eq (Get-ItemProperty $run -ErrorAction SilentlyContinue).'synaps-avatar') 'the Run value is gone'
Check (-not (Test-Path $lnk)) 'the shortcut is gone'
Check (Test-Path (Join-Path $pack 'sprites.json')) 'his own pack folder is untouched'

Remove-Item -Recurse -Force $pack, $notPack -ErrorAction SilentlyContinue
if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'all installer checks passed'
