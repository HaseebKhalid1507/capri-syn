# Installs, checks and uninstalls her installer, unattended, as his own user (CI runs it after
# build.ps1; it also runs as is in the capri-win VM). Her pack's paths: offline (Synaps and her art both
# fail, Setup still finishes and says why), a CapriCore that doesn't match its pins, /PACK=download over
# a pack of his (moved aside, kept), again (up to date), then a stand-in pack (a sprites.json she rejects)
# copied over the one Setup built (replaced whole). Synaps is left out here (/SYNAPS=skip) but offline:
# the end-to-end job (test-setup-e2e.ps1) installs it.
#
#   .\packaging\windows\test-install.ps1 -Setup .\packaging\windows\Output\capri-syn-setup-....exe
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Logs beside the checks.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSReviewUnusedParameter', '', Justification = 'Install reads it in script scope.')]
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
$cache = Join-Path $data 'installer'
$exe = Join-Path $app 'synaps-avatar.exe'
$herLog = Join-Path $data 'synaps-avatar.log'
$marker = Join-Path $packTo '.capri-syn-pack'
$synaps = Join-Path $(if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' } else { Join-Path $env:USERPROFILE '.cargo\bin' }) 'synaps.exe'

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

# Her installer, unattended; its log in $script:SetupLog (its steps' lines shown).
function Install([string[]]$more, [int]$secs = 180) {
    $log = Join-Path ([IO.Path]::GetTempPath()) "sa-setup-$PID.log"
    Remove-Item -Force $log -ErrorAction SilentlyContinue
    $code = RunFor $Setup (@('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$log`"") + $more) $secs
    $script:SetupLog = if (Test-Path $log) { Get-Content -Raw $log } else { '' }
    $script:SetupLog -split "`r?`n" | Where-Object { $_ -match '(step |synaps|synaps-check|pack|daemon|finished): ' } | ForEach-Object { Write-Host "  setup: $_" }
    if (-not ($script:SetupLog -match 'step ')) { Get-Content $log -Tail 15 -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "  setup: $_" } }
    return $code
}
# Her --check on a pack: exit code and her log's line.
function Test-HerPack([string]$Dir) {
    $before = if (Test-Path $herLog) { (Get-Item $herLog).Length } else { 0 }
    $c = Start-Process $exe -ArgumentList "--pack `"$Dir`" --check" -Wait -PassThru -WindowStyle Hidden
    $t = if (Test-Path $herLog) { $all = Get-Content -Raw $herLog; if ($all.Length -ge $before) { $all.Substring([int]$before) } else { $all } } else { '' }
    return @{ Code = $c.ExitCode; Line = [regex]::Match($t, 'pack `[^`]*` ok: \d+ frames[^,]*').Value }
}

# A folder that isn't a pack is refused, and nothing is installed.
$code = Install @("/PACK=`"$notPack`"", '/SYNAPS=skip')
Check ($code -ne 0 -and -not (Test-Path (Join-Path $app 'synaps-avatar.exe'))) "a folder without sprites.json is refused (exit $code)"

# Offline: GitHub, python.org and PyPI sent nowhere (the hosts file; CI's runner is an admin). Synaps
# and her art both fail, Setup still finishes, she's installed without them, and the log says why
# and how to try again.
$hosts = Join-Path $env:SystemRoot 'System32\drivers\etc\hosts'
$hostsWas = [IO.File]::ReadAllText($hosts)
$names = 'github.com', 'api.github.com', 'codeload.github.com', 'raw.githubusercontent.com', 'objects.githubusercontent.com',
    'release-assets.githubusercontent.com', 'www.python.org', 'pypi.org', 'files.pythonhosted.org'
try {
    [IO.File]::AppendAllText($hosts, "`r`n" + (($names | ForEach-Object { "0.0.0.0 $_ # capri-syn test-install" }) -join "`r`n") + "`r`n")
    ipconfig /flushdns | Out-Null
    $code = Install @() 300
} finally {
    [IO.File]::WriteAllText($hosts, $hostsWas)
    ipconfig /flushdns | Out-Null
}
Check ($code -eq 0) "offline, with its defaults: Setup still exits 0 ($code)"
Check (Test-Path $exe) 'she is installed'
Check (-not (Test-Path (Join-Path $packTo 'sprites.json'))) 'with no pack'
Check (-not (Test-Path $synaps)) 'and no Synaps'
Check ($script:SetupLog -match 'synaps: failed: [^\r\n]*offline') "the log says why Synaps didn't come: $([regex]::Match($script:SetupLog, 'synaps: failed: [^\r\n]*').Value)"
Check ($script:SetupLog -match 'pack: failed: [^\r\n]*offline') "and why her art didn't: $([regex]::Match($script:SetupLog, 'pack: failed: [^\r\n]*').Value)"
Check ($script:SetupLog -match 'steps\.ps1" pack') 'and the command that tries again'

# A CapriCore that doesn't match its pins (in the download cache): refused, nothing installed from it,
# the bad copy deleted so the next try downloads it again.
$bad = Join-Path $cache 'CapriCore-bc93892'
New-Item -ItemType Directory -Force (Join-Path $bad 'docs') | Out-Null
Set-Content (Join-Path $bad 'docs\character-assets.md') 'not CapriCore'
$code = Install @('/PACK=download', '/SYNAPS=skip') 900
Check ($code -eq 0) "a CapriCore that doesn't match its pins: Setup still exits 0 ($code)"
Check (-not (Test-Path (Join-Path $packTo 'sprites.json'))) 'no pack from it'
Check ($script:SetupLog -match "pack: failed: CapriCore's files don't match their pins") "the log says so: $([regex]::Match($script:SetupLog, 'pack: failed: [^\r\n]*').Value)"
Check (-not (Test-Path $bad)) 'and the bad copy is gone'

# /PACK=download over a pack of his (no marker): his goes aside, kept; hers is built, checked, marked.
New-Item -ItemType Directory -Force $packTo | Out-Null
Set-Content (Join-Path $packTo 'made-by-hand.txt') 'his own pack'
$code = Install @('/PACK=download', '/SYNAPS=skip') 900
Check ($code -eq 0) "/PACK=download exits 0 ($code)"
Check ($script:SetupLog -notmatch 'pack: failed') "her art came$(if ($script:SetupLog -match 'pack: failed: [^\r\n]*') { ': ' + $Matches[0] })"
Check (Test-Path (Join-Path $packTo 'sprites.json')) "sprites.json in $packTo"
Check (Test-Path (Join-Path $packTo 'anim\lachy\LICENSE.md')) "lachy's art, with its licence"
Check (Test-Path $marker) 'marked as the steps'' own (.capri-syn-pack)'
$hc = Test-HerPack $packTo
Check ($hc.Code -eq 0 -and $hc.Line) "her --pack $packTo --check: $($hc.Line) (exit $($hc.Code))"
$aside = @(Get-ChildItem -Directory -Path (Split-Path $packTo) -Filter 'capri.pre-install-*' | ForEach-Object { $_.FullName })
Check ($aside.Count -eq 1 -and (Test-Path (Join-Path "$($aside[0])" 'made-by-hand.txt'))) "his pack went aside, whole ($($aside -join ', '))"
Check ($script:SetupLog -match 'Required Notice: ') "lachy's Required Notice lines were said"
Check (-not (Test-Path $cache)) 'the download cache is gone after the build'

# Again: up to date, nothing downloaded.
$code = Install @('/PACK=download', '/SYNAPS=skip') 300
Check ($code -eq 0 -and $script:SetupLog -match 'up to date \(') "/PACK=download again: up to date (exit $code)"

# A pack folder of his (a stand-in), over the one Setup built: that one is replaced whole.
$code = Install @("/PACK=`"$pack`"", '/SYNAPS=skip', '/TASKS=autostart')
Check ($code -eq 0) "installs unattended (exit $code)"
Check (Test-Path (Join-Path $app 'synaps-avatar.exe')) "her exe in $app"
Check (Test-Path (Join-Path $app 'unins000.exe')) 'an uninstaller'
Check (Test-Path (Join-Path $packTo 'sprites.json')) "the pack copied to $packTo"
Check (Test-Path (Join-Path $packTo 'poses\stand-in.txt')) "with its folders"
Check (-not (Test-Path (Join-Path $packTo 'anim'))) 'the pack Setup built is replaced whole (nothing of it left)'
Check (@(Get-ChildItem -Directory -Path (Split-Path $packTo) -Filter 'capri.pre-install-*').Count -eq 1) 'and not moved aside (it was the steps'' own)'
Check ((Get-ItemProperty $aumid -ErrorAction SilentlyContinue).DisplayName -eq 'synaps-avatar') 'her AUMID registered (DisplayName)'
Check ((Get-ItemProperty $aumid -ErrorAction SilentlyContinue).IconUri -eq (Join-Path $app 'synaps-avatar.png')) 'with her icon (IconUri)'
Check (Test-Path (Join-Path $app 'synaps-avatar.png')) 'the icon file'
Check ((Get-ItemProperty $run -ErrorAction SilentlyContinue).'synaps-avatar' -eq "`"$(Join-Path $app 'synaps-avatar.exe')`"") 'autostart (the Run key), when asked'
Check (Test-Path $lnk) 'a Start-menu shortcut'
# Settings > Apps shows EstimatedSize (KB). Sizing the pack before his pick once walked all of C:.
$size = (Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -eq 'capri-syn' } | Select-Object -First 1).EstimatedSize
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
$left = @(Get-ChildItem -Recurse -Force $data -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notlike "$($aside[0])*" } | ForEach-Object { $_.FullName })
Check ($left.Count -eq 0) "nothing of hers is left in $data (pack, log, state, cache)$(if ($left) { ': ' + ($left -join ', ') })"
Check ($aside.Count -eq 1 -and (Test-Path (Join-Path "$($aside[0])" 'made-by-hand.txt'))) 'his pack from before stays'
Check ((Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -eq 'capri-syn' }).Count -eq 0) 'her Apps entry (capri-syn) is gone'
foreach ($a in $aside) { Remove-Item -Recurse -Force -LiteralPath $a }
Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
Check (-not (Test-Path $aumid)) 'her AUMID key is gone'
Check ($null -eq (Get-ItemProperty $run -ErrorAction SilentlyContinue).'synaps-avatar') 'the Run value is gone'
Check (-not (Test-Path $lnk)) 'the shortcut is gone'
Check (Test-Path (Join-Path $pack 'sprites.json')) 'his own pack folder is untouched'

Remove-Item -Recurse -Force $pack, $notPack -ErrorAction SilentlyContinue
if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'all installer checks passed'
