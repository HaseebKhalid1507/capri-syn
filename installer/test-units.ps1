# install.ps1's path guard and recorded folders on their own, on real Windows paths (CI's
# windows-installer job); and steps.ps1's Set-Pack, when the new pack can't go in. Their functions
# are loaded from the scripts' syntax trees: nothing runs.
#
#   .\installer\test-units.ps1
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'install.ps1''s own functions print with it.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidOverwritingBuiltInCmdlets', '', Justification = 'Move-Item, failing on cue: Set-Pack''s rollback.')]
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

# Which of her runs is his own: a command line with arguments after her program, quoted or not.
foreach ($c in @(
        @('"C:\Users\a b\AppData\Local\Programs\synaps-avatar\synaps-avatar.exe"', ''),
        @('"C:\Users\a b\AppData\Local\Programs\synaps-avatar\synaps-avatar.exe" ', ''),
        @('C:\Users\ab\AppData\Local\Programs\synaps-avatar\synaps-avatar.exe', ''),
        @('"C:\Users\a b\synaps-avatar.exe" --socket \\.\pipe\synaps', '--socket \\.\pipe\synaps'),
        @('C:\x\synaps-avatar.exe --profile work', '--profile work'),
        @('"C:\x\synaps-avatar.exe"--session capri', '--session capri'),
        @('', ''))) {
    Check ((Get-CommandArgs $c[0]) -ceq $c[1]) "Get-CommandArgs '$($c[0])' -> '$($c[1])'"
}

# How a stop names a run: in another sign-in session (his desktop, seen from ssh), and his own.
$l = Get-RunLabel ([pscustomobject]@{ Id = 42; Session = 1; Here = $false; Line = '"C:\x\synaps-avatar.exe" --profile work' })
Check ($l -eq 'pid 42 in your other sign-in session (1), as you started her: "C:\x\synaps-avatar.exe" --profile work') "Get-RunLabel: another session, his own run ($l)"
$l = Get-RunLabel ([pscustomobject]@{ Id = 7; Session = 2; Here = $true; Line = '' })
Check ($l -eq 'pid 7') "Get-RunLabel: here, bare ($l)"

# steps.ps1's Set-Pack: a new pack that can't go in (a folder in use) leaves the one that was there
# in place, his own or the steps', and the new one where it was. Move-Item fails on cue for it.
$stepsAst = [Management.Automation.Language.Parser]::ParseFile((Join-Path (Split-Path -Parent $PSScriptRoot) 'packaging\windows\steps.ps1'), [ref]$null, [ref]$null)
foreach ($name in 'Set-Pack', 'Say', 'Good', 'Warn', 'Stop-Step', 'Get-SignInPage') {
    $fn = $stepsAst.Find({ param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name }, $true)
    . ([scriptblock]::Create($fn.Extent.Text))
}
# Each account's sign-in page (Get-SignInPage's), and a model's id, as steps.ps1 sets them.
foreach ($v in '$script:Reopen', '$script:ModelId') {
    $a = $stepsAst.Find({ param($n) $n -is [Management.Automation.Language.AssignmentStatementAst] -and "$($n.Left)" -eq $v }, $true)
    . ([scriptblock]::Create($a.Extent.Text))
}
function Move-Item([string]$LiteralPath, [Parameter(Position = 1)] [string]$Destination) {
    if ($script:FailMove -and $LiteralPath -eq $script:FailMove) { throw 'The process cannot access the file because it is being used by another process. (a test)' }
    Microsoft.PowerShell.Management\Move-Item -LiteralPath $LiteralPath -Destination $Destination
}
# The sign-in page steps.ps1 login opens again whole (Synaps' line after "If the browser didn't
# open, visit this URL:"): https on exactly the provider's host and path, with an & that cmd cut; its
# AbsoluteUri. Nothing else: another provider's, another host or path, a user@, a port, a \, a #,
# http.
$claude = 'https://claude.ai/oauth/authorize?code=true&client_id=9d1c&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&state=x'
foreach ($c in @(
        @($claude, $claude, 'anthropic'),
        @('https://CLAUDE.AI/oauth/authorize?a=1&b=2', 'https://claude.ai/oauth/authorize?a=1&b=2', 'anthropic'),
        @('https://auth.openai.com/oauth/authorize?response_type=code&client_id=app_X&state=s9', 'https://auth.openai.com/oauth/authorize?response_type=code&client_id=app_X&state=s9', 'openai-codex'),
        @('https://accounts.google.com/o/oauth2/v2/auth?a=1&b=2', 'https://accounts.google.com/o/oauth2/v2/auth?a=1&b=2', 'google-gemini'),
        # Device sign-ins: nothing to reopen. Grok: not here. A key service: no page.
        @('https://github.com/login/device?a=1&b=2', '', 'github-copilot'),
        @('https://www.kimi.com/code/authorize_device?a=1&b=2', '', 'kimi-code'),
        @('https://claude.ai/oauth/authorize?a=1&b=2', '', 'xai-auth'),
        @('https://claude.ai/oauth/authorize?a=1&b=2', '', 'groq'),
        # Another provider's page.
        @($claude, '', 'openai-codex'),
        @('https://auth.openai.com/oauth/authorize?a=1&b=2', '', 'anthropic'),
        # Its site, another page: a redirect through Google, a path that isn't the sign-in's.
        @('https://www.google.com/url?q=https://accounts.google.com/o/oauth2/v2/auth&sa=D', '', 'google-gemini'),
        @('https://accounts.google.com/url?q=x&sa=D', '', 'google-gemini'),
        @('https://claude.ai/oauth/authorizer?a=1&b=2', '', 'anthropic'),
        @('https://claude.ai/oauth/authorize/more?a=1&b=2', '', 'anthropic'),
        @('https://claude.ai/oauth/authorize?a=1&b=2#x', '', 'anthropic'),
        @('https://claude.ai/logout?a=1&b=2', '', 'anthropic'),
        @('https://console.anthropic.com/oauth/authorize?a=1&b=2', '', 'anthropic'),
        # Another host that looks like it.
        @('https://claude.ai.evil.example/oauth/authorize?a=1&b=2', '', 'anthropic'),
        @('https://notclaude.ai/oauth/authorize?a=1&b=2', '', 'anthropic'),
        @('https://evil.example/claude.ai/oauth/authorize?a=1&b=2', '', 'anthropic'),
        @('https://evil.com\.claude.ai/?a=1&b=2', '', 'anthropic'),
        @('https://claude.ai\oauth\authorize?a=1&b=2', '', 'anthropic'),
        # A user@, a port (even 443), not https, two words, nothing cut.
        @('https://claude.ai@evil.example/oauth/authorize?a=1&b=2', '', 'anthropic'),
        @('https://me@claude.ai/oauth/authorize?a=1&b=2', '', 'anthropic'),
        @('https://claude.ai:8443/oauth/authorize?a=1&b=2', '', 'anthropic'),
        @('https://claude.ai:443/oauth/authorize?a=1&b=2', '', 'anthropic'),
        @('http://claude.ai/oauth/authorize?a=1&b=2', '', 'anthropic'),
        @('https://claude.ai/oauth/authorize?a=1&b=2 and more', '', 'anthropic'),
        @('https://claude.ai/oauth/authorize?a=1', '', 'anthropic'),
        @('file:///C:/Windows/System32/calc.exe?a&b', '', 'anthropic'),
        @('not a url', '', 'anthropic'))) {
    $got = Get-SignInPage $c[0] $c[2]
    Check ($got -ceq $c[1]) "Get-SignInPage $($c[0]) for $($c[2]): $(if ($c[1]) { 'opened' } else { 'never' })$(if ($got -cne $c[1]) { " (got '$got')" })"
}
# A model's id, as it may reach a command line: nothing with a quote, a space, a ; or a $ in it.
foreach ($c in @(@('qwen3:8b', $true), @('qwen/qwen3-8b', $true), @('hf.co/bartowski/Llama-3.2-3B-Instruct-GGUF:Q4_K_M', $true), @('model@v1+fast', $true),
        @('a"b', $false), @('a b', $false), @('a;b', $false), @('$(calc)', $false), @("a`nb", $false), @("ab`n", $false), @('', $false))) {
    Check (($c[0] -cmatch $script:ModelId) -eq $c[1]) "a model id $($c[0] -replace "`n", '\n'): $(if ($c[1]) { 'taken' } else { 'refused' })"
}

$script:Marker = '.capri-syn-pack'
$script:Ours = $false
$packs = Join-Path $tmp 'packs'
$Out = Join-Path $packs 'capri'
function Add-Pack([string]$Dir, [string]$File, [switch]$Theirs) {
    New-Item -ItemType Directory -Force $Dir | Out-Null
    Set-Content -LiteralPath (Join-Path $Dir $File) -Value 'x'
    if ($Theirs) { Set-Content -LiteralPath (Join-Path $Dir $script:Marker) -Value 'source=old' }
}
foreach ($case in @(@{ Name = 'his own pack'; Theirs = $false }, @{ Name = 'a pack the steps made'; Theirs = $true })) {
    if (Test-Path -LiteralPath $packs) { Remove-Item -Recurse -Force -LiteralPath $packs }
    Add-Pack $Out 'before.txt' -Theirs:$case.Theirs
    $new = "$Out.new"
    Add-Pack $new 'sprites.json'
    $script:FailMove = $new
    $said = ''
    try { Set-Pack $new 'fp' 'test' 6>$null } catch { $said = "$_" }
    $script:FailMove = $null
    Check ($said -match "^step: her new pack couldn't go into") "Set-Pack, $($case.Name), the new one in use: it says so ($said)"
    Check ((Test-Path -LiteralPath (Join-Path $Out 'before.txt')) -and -not (Test-Path -LiteralPath (Join-Path $Out 'sprites.json'))) "Set-Pack, $($case.Name): the one before is back in place"
    Check (@(Get-ChildItem -LiteralPath $packs -Directory | Where-Object { $_.Name -ne 'capri' -and $_.Name -ne 'capri.new' }).Count -eq 0) "Set-Pack, $($case.Name): nothing left aside ($(@(Get-ChildItem -LiteralPath $packs -Name) -join ', '))"
    Check (Test-Path -LiteralPath (Join-Path $new 'sprites.json')) "Set-Pack, $($case.Name): the new one is where it was"
    # And with nothing in the way: in, the steps' old one gone, his aside.
    Set-Pack $new 'fp' 'test' 6>$null
    Check ((Test-Path -LiteralPath (Join-Path $Out 'sprites.json')) -and (Test-Path -LiteralPath (Join-Path $Out $script:Marker))) "Set-Pack, $($case.Name): then in, marked"
    $aside = @(Get-ChildItem -LiteralPath $packs -Directory -Filter 'capri.pre-install-*')
    Check ($aside.Count -eq [int](-not $case.Theirs) -and -not (Test-Path -LiteralPath "$Out.old")) "Set-Pack, $($case.Name): $(if ($case.Theirs) { 'replaced whole' } else { 'his moved aside' })"
}

Remove-Item -Recurse -Force -LiteralPath $tmp
if ($fails.Count) { throw "$($fails.Count) check(s) failed: $($fails -join '; ')" }
Write-Output 'all unit checks passed'
