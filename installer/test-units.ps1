# install.ps1's path guard and recorded folders on their own, on real Windows paths (CI's
# windows-installer job). Its functions are loaded from the script's syntax tree: nothing runs.
#
#   .\installer\test-units.ps1
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'install.ps1''s own functions print with it.')]
param()
$ErrorActionPreference = 'Stop'
$fails = [System.Collections.Generic.List[string]]::new()
function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Output "ok   $what" } else { Write-Output "FAIL $what"; $fails.Add($what) }
}
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path (Split-Path -Parent $PSScriptRoot) 'install.ps1'), [ref]$null, [ref]$null)
foreach ($fn in $ast.FindAll({ $args[0] -is [Management.Automation.Language.FunctionDefinitionAst] }, $true)) {
    . ([scriptblock]::Create($fn.Extent.Text))
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) "capri-units-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null
$script:Dry = $false
$script:Receipt = Join-Path $tmp 'install.receipt'
$prefix = Join-Path $tmp 'My Apps\synaps-avatar'
$script:Prefixes = @($prefix)
$script:Data = Join-Path $env:LOCALAPPDATA 'synaps-avatar'
$script:Conf = Join-Path $env:APPDATA 'synaps-avatar'
$script:Lnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\synaps-avatar.lnk'

$cases = @(
    @("$prefix\synaps-avatar.exe", 'file', $true),
    @((Join-Path $tmp 'My'), 'file', $false),
    @($prefix, 'dir', $false),
    @($prefix, 'rmdir', $true),
    @((Join-Path $tmp 'My Apps'), 'rmdir', $true),
    @($env:USERPROFILE, 'rmdir', $false),
    @($env:LOCALAPPDATA, 'rmdir', $false),
    @($env:APPDATA, 'dir', $false),
    @('C:\', 'rmdir', $false),
    @("$env:USERPROFILE\Documents", 'dir', $false),
    @("$env:LOCALAPPDATA\synaps-avatar\packs\capri", 'dir', $true),
    @("$env:LOCALAPPDATA\synaps-avatar\installer", 'dir', $true),
    @("$prefix\voice", 'dir', $true),
    @("$env:LOCALAPPDATA\synaps-avatar\packs\mine", 'dir', $false),
    @("$env:LOCALAPPDATA\synaps-avatar\packs", 'dir', $false),
    @("$prefix\anything", 'dir', $false),
    @("$prefix\anything", 'file', $true),
    @("$env:LOCALAPPDATA\synaps-avatar\..\..\Roaming", 'dir', $false),
    @("$env:LOCALAPPDATA\capri-voice", 'dir', $true),
    @("$env:LOCALAPPDATA\capri-voice", 'file', $false),
    @($script:Lnk, 'file', $true),
    @("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\other.lnk", 'file', $false),
    @('relative\synaps-avatar.exe', 'file', $false),
    @("$prefix\a`nb", 'file', $false)
)
foreach ($c in $cases) {
    $got = Test-Removable $c[0] $c[1]
    Check ($got -eq $c[2]) "Test-Removable $($c[0] -replace "`n", '\n') ($($c[1])) = $got"
}

# The folders New-RecordedPath makes are recorded as rmdir (removed only if empty), never as dir.
New-RecordedPath $prefix
$r = @(Get-Content -LiteralPath $script:Receipt)
Check (Test-Path -LiteralPath $prefix) 'New-RecordedPath makes the folder'
Check (($r -join '|') -eq "rmdir`t$(Join-Path $tmp 'My Apps')|rmdir`t$prefix") "and records each it made, outermost first ($($r -join ' | '))"
New-RecordedPath $prefix
Check (@(Get-Content -LiteralPath $script:Receipt).Count -eq 2) 'nothing more when it is there'
Check (Test-Recorded 'rmdir' $prefix) 'Test-Recorded finds an entry'
Check (-not (Test-Recorded 'rmdir' $tmp)) 'and not one that is not there'

# Add-Receipt's dedup compares whole lines: a path that is the start of one already there still goes in.
Remove-Item -LiteralPath $script:Receipt
Add-Receipt @('file', "$prefix\a b.exe")
Add-Receipt @('file', "$prefix\a")
Add-Receipt @('file', "$prefix\a")
$r = @(Get-Content -LiteralPath $script:Receipt)
Check (($r -join '|') -eq "file`t$prefix\a b.exe|file`t$prefix\a") "Add-Receipt: whole lines, once each ($($r -join ' | '))"

Remove-Item -Recurse -Force -LiteralPath $tmp
if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'all unit checks passed'
