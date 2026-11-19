# capri-syn's install steps on Windows, the one copy of them: install.ps1 runs them (its steps 1, 3, 4
# and 7), and so does her installer (capri-syn-setup, synaps-avatar.iss) once her binary is in place.
# One or more steps per run, in order, stopping at the first that fails ("steps.ps1 synaps daemon"):
#
#   steps.ps1 synaps-check   is Synaps here (`synaps` on PATH, else ~\.cargo\bin\synaps.exe) and at
#                            least installer/manifest's synaps_min? Reads only.
#   steps.ps1 synaps         the same, then, when it's missing or older: the latest release's
#                            synaps-installer.ps1, checked against GitHub's sha256 of the asset (the
#                            manifest's pinned release and hash when GitHub's API doesn't answer), run
#                            unattended. Its installer puts ~\.cargo\bin on his user Path.
#   steps.ps1 daemon         the Synaps daemon now (`synaps daemon --detach` unless it runs) and at
#                            sign-in: Run\synaps-daemon through conhost --headless, so no console window
#                            opens (a Run value of his own that starts it is left as it is).
#   steps.ps1 pack           her art pack (below).
#   steps.ps1 mics           the microphones Windows has (its capture endpoints that work). Reads only.
#   steps.ps1 voice-check    where her voice would come from (-VoiceFrom, else the latest release), or
#                            none yet. Reads only.
#   steps.ps1 voice          her voice: its installer (-VoiceFrom FILE or URL, else the latest
#                            release's; a .sha256 beside it checked), or -Bundle DIR already in place;
#                            then its models (~375 MB, each pinned); then his choices (-Mic, -WakeWord,
#                            -ReadAloud) written to settings.toml by her own settings writer
#                            (synaps-avatar --set).
#   steps.ps1 login          `synaps login --provider claude` with no window: his browser opens, and he
#                            signs in there (given 10 minutes).
#   steps.ps1 synaps-remove  Synaps' daemon stopped, synaps.exe and its install record removed. His login
#                            and sessions (~\.synaps-cli) stay.
#   steps.ps1 remove         her uninstaller's part: what these steps made that isn't in its own record
#                            (her pack when it holds their marker, their Run\synaps-daemon, her voice).
#
# Her pack: CPython's embeddable package and pip (both pinned, SHA-256 checked) in -Cache, pip_pack
# (fonttools) into it; CapriCore at capricore_commit (GitHub's tarball); then tools/make-pack with
# `--lachy auto`, every file checked against tools/*.sha256; her --check on the result; then into
# place. A pack these steps put there (it holds .capri-syn-pack) is replaced whole; any other (his own)
# is moved aside to <pack>.pre-install-<date>, never deleted. One built from the same inputs is left
# as it is. -From DIR places that pack folder instead of building one. Her art is never in this repo
# or in her installer: it's downloaded here, on his machine. Never mochi's, unless -LocalArt
# (install.ps1 --with-local-art, LOCAL PERSONAL USE ONLY).
#
# What it does is said for a person ("   + done", "   ! careful", "   text"); results are a key at the
# left edge ("synaps: ok VERSION PATH", "brain: no", "daemon: running 3", "runvalue: set"); a failure is
# "failed: why", in plain words, and exit 1, with nothing left half done.
#
# Its tree: this folder's installer\manifest, tools\ and packs\ (her installer's copy), else the repo's
# (packaging\windows\..\..). Windows PowerShell 5.1 and PowerShell 7. ASCII only.
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingWriteHost', '', Justification = 'Its lines are read as they come: install.ps1 shows them, her installer its progress page.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = '-DryRun is the what-if, as in install.ps1.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Show-Notices reads as install.ps1 named it.')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSReviewUnusedParameter', '', Justification = 'The steps read them in script scope.')]
param(
    [Parameter(Position = 0, Mandatory, ValueFromRemainingArguments)] [ValidateSet('synaps-check', 'synaps', 'daemon', 'pack', 'mics', 'voice-check', 'voice', 'login', 'synaps-remove', 'remove')] [string[]]$Step,
    [string]$Synaps = '',
    [string]$Out = (Join-Path $env:LOCALAPPDATA 'synaps-avatar\packs\capri'),
    [string]$Exe = '',
    [string]$From = '',
    [string]$Cache = (Join-Path $env:LOCALAPPDATA 'synaps-avatar\installer'),
    [switch]$KeepCache,
    [switch]$LocalArt,
    [switch]$Ours,
    [string]$VoiceFrom = '',
    [string]$Bundle = '',
    [string]$Mic = '',
    [ValidateSet('on', 'off')] [string]$WakeWord = 'on',
    [ValidateSet('on', 'off')] [string]$ReadAloud = 'on',
    [switch]$DryRun
)

# Everything below runs from the last line: a copy cut off in a download doesn't parse, so never runs half.
function Invoke-Steps {
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# Windows PowerShell started from PowerShell 7 (or from a setup started there) inherits 7's module
# path, and then can't load its own Microsoft.PowerShell.Utility, Archive and so on. Its own first.
if ($PSVersionTable.PSEdition -eq 'Desktop') {
    $own = @((Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'WindowsPowerShell\Modules'),
        (Join-Path $env:ProgramFiles 'WindowsPowerShell\Modules'), (Join-Path $PSHOME 'Modules'))
    $env:PSModulePath = (@($own) + @("$env:PSModulePath" -split ';' | Where-Object { $_ -and $_ -notmatch '\\PowerShell\\7|\\powershell\\Modules$' -and $own -notcontains $_ })) -join ';'
}
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
# Read as UTF-8 by both callers (paths can have any letter in them).
if ([Console]::IsOutputRedirected) { try { [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false } catch { $null = $_ } }
$script:Root = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'installer\manifest')) { $PSScriptRoot } else { (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path }
$script:Marker = '.capri-syn-pack'
$script:Tmp = Join-Path ([IO.Path]::GetTempPath()) "capri-steps-$PID"
$script:RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$script:CargoBin = if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin' } else { Join-Path $env:USERPROFILE '.cargo\bin' }
try {
    $script:M = Read-Manifest
    foreach ($s in $Step) {
        switch ($s) {
            'synaps-check' { Show-Synaps (Find-Synaps) }
            'synaps' { Install-Synaps }
            'daemon' { Start-Daemon }
            'pack' { if ($From) { Copy-Pack } else { Build-Pack } }
            'mics' { Show-Mics }
            'voice-check' { $v = Find-Voice; Result "voicefrom: $($v.Kind) $($v.Where)" }
            'voice' { Install-Voice }
            'login' { Invoke-Login }
            'synaps-remove' { Remove-Synaps }
            'remove' { Remove-Hers }
        }
    }
    exit 0
} catch {
    $why = "$($_.Exception.Message)"
    if ($why -like 'step: *') { $why = $why.Substring(6) } else { $why = "something went wrong: $why ($($_.InvocationInfo.ScriptLineNumber))" }
    Write-Host "failed: $why"
    exit 1
} finally {
    Remove-Item -Recurse -Force -LiteralPath $script:Tmp -ErrorAction SilentlyContinue
}
}

# ---- small tools ------------------------------------------------------------------------------

function Say([string]$Text) { Write-Host "   $Text" }
function Good([string]$Text) { Write-Host "   + $Text" }
function Warn([string]$Text) { Write-Host "   ! $Text" }
function Result([string]$Text) { Write-Host $Text }
# The step stops: the reason in plain words (the last line, "failed: ...").
function Stop-Step([string]$Why) { throw "step: $Why" }

function Read-Manifest {
    $f = Join-Path $script:Root 'installer\manifest'
    if (-not (Test-Path -LiteralPath $f)) { Stop-Step "no installer\manifest beside it ($f)" }
    $m = @{}
    foreach ($line in (Get-Content -LiteralPath $f)) {
        $l = $line.Trim()
        if (-not $l -or $l.StartsWith('#')) { continue }
        $eq = $l.IndexOf('=')
        if ($eq -gt 0) { $m[$l.Substring(0, $eq)] = $l.Substring($eq + 1) }
    }
    $m
}
function Get-Key([string]$Key) {
    $v = $script:M[$Key]
    if (-not $v) { Stop-Step "installer/manifest has no $Key" }
    $v
}

function Get-Headers {
    $h = @{ 'User-Agent' = 'capri-syn-install' }
    if ($env:GITHUB_TOKEN) { $h['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    $h
}
function Get-Text([string]$Url) {
    $r = Invoke-WebRequest -UseBasicParsing -Uri $Url -Headers (Get-Headers) -TimeoutSec 30
    if ($r.Content -is [byte[]]) { [Text.Encoding]::UTF8.GetString($r.Content) } else { [string]$r.Content }
}
# Why a download failed, in a few words, and whether that looks like no network at all.
function Get-NetWhy($E) {
    $w = $E
    while ($w -and -not ($w -is [Net.WebException]) -and $w.InnerException) { $w = $w.InnerException }
    if ($w -is [Net.WebException]) {
        $off = "$($w.Status)" -ne 'ProtocolError'
        return @{ Text = $w.Message; Offline = $off }
    }
    # PowerShell 7: an HTTP answer (404, 500) is an HttpResponseException: the network is there.
    if ($E.GetType().Name -eq 'HttpResponseException') { return @{ Text = $E.Message; Offline = $false } }
    while ($E.InnerException) { $E = $E.InnerException }
    return @{ Text = $E.Message; Offline = $true }
}
# A file from the web into To, saying how far it is every few MB (her installer shows those lines).
function Get-Url([string]$Url, [string]$To, [string]$What) {
    $part = "$To.part"
    New-Item -ItemType Directory -Force (Split-Path -Parent $To) | Out-Null
    try {
        $req = [Net.WebRequest]::Create($Url)
        $req.UserAgent = 'capri-syn-install'
        $req.Timeout = 30000
        $req.ReadWriteTimeout = 60000
        $res = $req.GetResponse()
        try {
            $len = $res.ContentLength
            $in = $res.GetResponseStream()
            $fs = [IO.File]::Create($part)
            try {
                $buf = New-Object byte[] 262144
                $done = 0L
                $next = 0L
                while (($n = $in.Read($buf, 0, $buf.Length)) -gt 0) {
                    $fs.Write($buf, 0, $n)
                    $done += $n
                    if ($done -ge $next) {
                        $of = if ($len -gt 0) { " of $([math]::Round($len / 1MB, 1)) MB" } else { ' MB' }
                        Say "$($What): $([math]::Round($done / 1MB, 1))$of"
                        $next = $done + 4MB
                    }
                }
            } finally { $fs.Dispose(); $in.Dispose() }
        } finally { $res.Dispose() }
    } catch {
        Remove-Item -Force -LiteralPath $part -ErrorAction SilentlyContinue
        $w = Get-NetWhy $_.Exception
        $host_ = ([Uri]$Url).Host
        if ($w.Offline) { Stop-Step "couldn't reach $host_ for $What ($($w.Text)). Is this PC offline?" }
        Stop-Step "couldn't download $What from $host_ ($($w.Text))"
    }
    Move-Item -Force -LiteralPath $part $To
}
function Get-Sha256([string]$Path) {
    $sha = [Security.Cryptography.SHA256]::Create()
    $fs = [IO.File]::OpenRead($Path)
    try { -join ($sha.ComputeHash($fs) | ForEach-Object { $_.ToString('x2') }) } finally { $fs.Dispose(); $sha.Dispose() }
}
function Test-Pin([string]$Path, [string]$Want, [string]$What) {
    $got = Get-Sha256 $Path
    if ($got -ne $Want.ToLowerInvariant()) {
        Remove-Item -Force -LiteralPath $Path
        Stop-Step "$What doesn't match its pinned SHA-256 (it is $($got.Substring(0, 12))..., pinned $($Want.Substring(0, 12))...), so it wasn't used"
    }
    Good "SHA-256 checked: $What"
}
# A native program, its lines said as they come (the last ones kept in $script:Last); its exit code.
function Invoke-Logged([string]$File, [string[]]$ArgList) {
    $script:Last = @()
    $ErrorActionPreference = 'Continue'
    & $File @ArgList 2>&1 | ForEach-Object {
        $l = "$_".TrimEnd()
        if ($l) {
            Say $l
            $script:Last = @(@($script:Last) + $l | Select-Object -Last 40)
        }
    }
    return $LASTEXITCODE
}

# ---- synaps -------------------------------------------------------------------------------------

function Get-SynapsVersion([string]$File) {
    $ErrorActionPreference = 'Continue'
    try { $o = (& $File --version 2>$null) | Select-Object -First 1 } catch { return $null }
    if ($o -match '(\d+\.\d+\.\d+)') { return $Matches[1] } else { return $null }
}
# Where Synaps is and whether it will do: State ok | old | missing.
function Find-Synaps {
    $cur = $Synaps
    if (-not $cur) { $cur = (Get-Command synaps -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source }
    $mine = Join-Path $script:CargoBin 'synaps.exe'
    if (-not $cur -and (Test-Path -LiteralPath $mine)) { $cur = $mine }
    if (-not $cur -or -not (Test-Path -LiteralPath $cur)) { return @{ State = 'missing'; Version = ''; Path = $mine } }
    $v = Get-SynapsVersion $cur
    $state = if ($v -and ([version]$v -ge [version](Get-Key 'synaps_min'))) { 'ok' } else { 'old' }
    return @{ State = $state; Version = "$v"; Path = $cur }
}
# A model to talk through: a login (OAuth or API key, both in auth.json), or a local model (in Synaps'
# config or her own [session]).
function Test-Brain {
    $h = if ($env:SYNAPS_BASE_DIR) { $env:SYNAPS_BASE_DIR } else { Join-Path $env:USERPROFILE '.synaps-cli' }
    $auth = Join-Path $h 'auth.json'
    if ((Test-Path -LiteralPath $auth) -and (Get-Item -LiteralPath $auth).Length -gt 0) { return $true }
    foreach ($cfg in @((Join-Path $h 'config'), (Join-Path $env:APPDATA 'synaps-avatar\settings.toml'))) {
        if ((Test-Path -LiteralPath $cfg) -and (Select-String -LiteralPath $cfg -Pattern '^\s*model\s*=\s*"?local/' -Quiet)) { return $true }
    }
    return $false
}
function Show-Synaps($S) {
    switch ($S.State) {
        'ok' { Good "synaps $($S.Version) ($($S.Path))" }
        'old' { Say "synaps $($S.Version) at $($S.Path) is older than $(Get-Key 'synaps_min')" }
        default { Say 'no Synaps here' }
    }
    if ($S.State -eq 'missing') { Result 'synaps: missing' } else { Result "synaps: $($S.State) $($S.Version) $($S.Path)" }
    Result "brain: $(if (Test-Brain) { 'yes' } else { 'no' })"
}

function Install-Synaps {
    $s = Find-Synaps
    if ($s.State -eq 'ok') { Show-Synaps $s; return }
    if ($s.State -eq 'old') { Say "synaps $($s.Version) at $($s.Path) is older than $(Get-Key 'synaps_min'): installing the latest" }
    else { Say 'installing Synaps with its own installer' }
    Result "was: $($s.State)"
    # The latest release, and GitHub's sha256 of its installer; else the manifest's pin.
    $repo = Get-Key 'synaps_repo'
    $tag = $null; $sha = $null
    try {
        $rel = Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers (Get-Headers) -TimeoutSec 30
        $tag = [string]$rel.tag_name
        $asset = $rel.assets | Where-Object { $_.name -eq 'synaps-installer.ps1' } | Select-Object -First 1
        if ($asset -and $asset.digest) { $sha = ([string]$asset.digest) -replace '^sha256:', '' }
    } catch { $tag = $null }
    if (-not $tag -or $sha -notmatch '^[0-9a-f]{64}$') {
        $tag = "v$(Get-Key 'synaps_pin')"; $sha = Get-Key 'synaps_installer_ps1_sha256'
        Say "GitHub's API didn't answer: the pinned $tag"
    }
    $url = "https://github.com/$repo/releases/download/$tag/synaps-installer.ps1"
    $mine = Join-Path $script:CargoBin 'synaps.exe'
    Result "release: $tag $url"
    if ($DryRun) {
        Say "`$ Invoke-WebRequest $url (its sha256: $sha), then: powershell -File synaps-installer.ps1"
        Result "synaps: would $($tag.TrimStart('v')) $mine"
        Result 'brain: no'
        return
    }
    $file = Join-Path $script:Tmp 'synaps-installer.ps1'
    Get-Url $url $file "Synaps' installer ($tag)"
    Test-Pin $file $sha "synaps-installer.ps1 ($tag)"
    Say "running Synaps' own installer ($tag)"
    $ps = (Get-Process -Id $PID).Path
    $rc = Invoke-Logged $ps @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $file)
    if ($rc) {
        $l = @($script:Last | Select-Object -Last 1)
        Stop-Step "Synaps' installer stopped (exit $rc): $l"
    }
    if (-not (Test-Path -LiteralPath $mine)) { Stop-Step "Synaps' installer ran, but there's no $mine" }
    $script:Synaps = $mine
    $s = Find-Synaps
    if ($s.State -ne 'ok') { Stop-Step "Synaps' installer ran, but $mine says version '$($s.Version)' (she needs $(Get-Key 'synaps_min') or newer)" }
    Show-Synaps $s
}

# The daemon status: Up, and its protocol.
function Get-DaemonStatus([string]$File) {
    $ErrorActionPreference = 'Continue'
    $t = (& $File daemon status 2>&1 | Out-String)
    $up = ($LASTEXITCODE -eq 0) -and ($t -match 'daemon: running')
    $p = if ($t -match 'protocol: (\d+)') { $Matches[1] } else { '?' }
    return @{ Up = $up; Protocol = $p; Text = $t.Trim() }
}
# One of these steps' own Run\synaps-daemon values (any Synaps path), as against one of his.
function Test-OurRunValue([string]$Value) { $Value -match '^"[^"]*\\conhost\.exe" --headless "[^"]*\\synaps\.exe" daemon --detach$' }

function Start-Daemon {
    $s = Find-Synaps
    if ($s.State -eq 'missing' -and -not $DryRun) { Stop-Step "there's no Synaps to run her brain's daemon (install it first: steps.ps1 synaps)" }
    $exe = $s.Path
    if ($DryRun) { Say "`$ $exe daemon --detach   (unless it runs)" }
    else {
        $st = Get-DaemonStatus $exe
        if ($st.Up) { Good 'the daemon runs already' }
        else {
            Say "`$ $exe daemon --detach"
            $rc = Invoke-Logged $exe @('daemon', '--detach')
            if ($rc) { Warn "synaps daemon --detach exited $rc" }
        }
    }
    # At sign-in: Synaps has no service on Windows; a Run value starts it. `synaps.exe` is a console
    # program: run as is, it opens a terminal that takes the foreground for a moment (seen in the VM).
    # A headless conhost gives it a console nobody sees (the daemon it starts has its own, hidden).
    # An EDR may flag that pattern: docs/WINDOWS.md ("conhost --headless and security software").
    $want = "`"$env:SystemRoot\System32\conhost.exe`" --headless `"$exe`" daemon --detach"
    $item = Get-ItemProperty -Path $script:RunKey -ErrorAction SilentlyContinue
    $vals = if ($item) { @($item.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' }) } else { @() }
    $theirs = $vals | Where-Object { $_.Name -ne 'synaps-daemon' -and "$($_.Value)" -match 'synaps' -and "$($_.Value)" -match 'daemon' } | Select-Object -First 1
    $cur = ($vals | Where-Object { $_.Name -eq 'synaps-daemon' } | Select-Object -First 1).Value
    if ($theirs) { Good "your Run\$($theirs.Name) starts the daemon at sign-in"; Result 'runvalue: his' }
    elseif ($cur -eq $want) { Good 'already set: Run\synaps-daemon'; Result 'runvalue: already' }
    elseif ($null -ne $cur -and -not (Test-OurRunValue $cur)) { Good "your own Run\synaps-daemon ($cur): left as it is"; Result 'runvalue: his' }
    else {
        Say "`$ reg add HKCU\...\Run /v synaps-daemon /d $want"
        if (-not $DryRun) { New-ItemProperty -Path $script:RunKey -Name 'synaps-daemon' -Value $want -PropertyType String -Force | Out-Null }
        Result 'runvalue: set'
    }
    if ($DryRun) { Result 'daemon: dry-run'; return }
    $st = $null
    for ($i = 0; $i -lt 20; $i++) {
        $st = Get-DaemonStatus $exe
        if ($st.Up) { break }
        Start-Sleep -Milliseconds 500
    }
    if (-not $st.Up) {
        Warn "the daemon isn't answering yet: $(($st.Text -split "`n")[0])"
        Result 'daemon: not-running'
        Result "brain: $(if (Test-Brain) { 'yes' } else { 'no' })"
        return
    }
    $proto = Get-Key 'synaps_protocol'
    if ($st.Protocol -ne '?' -and $st.Protocol -ne $proto) {
        Warn "the daemon speaks protocol $($st.Protocol); her client speaks $proto (exact match): she can't attach. Install synaps $(Get-Key 'synaps_pin'), or a capri-syn that speaks $($st.Protocol)"
    }
    Good "daemon running (protocol $($st.Protocol)): at sign-in too"
    Result "daemon: running $($st.Protocol)"
    Result "brain: $(if (Test-Brain) { 'yes' } else { 'no' })"
}

# Her installer's "sign in" box (and her Start-menu "Sign in to Claude"): Synaps' Claude login with no
# window. It opens his browser and waits on its localhost callback. Its stdin is a pipe held open: at
# end of input (a closed or null stdin) synaps 0.12's login gives up at once ("Manual input channel
# closed"), and after a sign-in it may wait on that read; so the pipe is closed once auth.json changes.
function Invoke-Login {
    $s = Find-Synaps
    if ($s.State -eq 'missing') { Stop-Step 'there is no Synaps to sign in' }
    $h = if ($env:SYNAPS_BASE_DIR) { $env:SYNAPS_BASE_DIR } else { Join-Path $env:USERPROFILE '.synaps-cli' }
    $auth = Join-Path $h 'auth.json'
    $was = if (Test-Path -LiteralPath $auth) { (Get-Item -LiteralPath $auth).LastWriteTimeUtc } else { [datetime]::MinValue }
    $pi = New-Object Diagnostics.ProcessStartInfo
    $pi.FileName = $s.Path
    $pi.Arguments = 'login --provider claude'
    $pi.UseShellExecute = $false
    $pi.CreateNoWindow = $true
    $pi.RedirectStandardInput = $true
    $pi.RedirectStandardOutput = $true
    $pi.RedirectStandardError = $true
    $p = [Diagnostics.Process]::Start($pi)
    $err = $p.StandardError.ReadToEndAsync()
    $null = $p.StandardOutput.ReadToEndAsync()
    Say 'your browser opens: sign in to Claude there (10 minutes)'
    $t0 = Get-Date
    $in = $false
    while (-not $p.HasExited -and ((Get-Date) - $t0).TotalMinutes -lt 10) {
        if (-not $in -and (Test-Path -LiteralPath $auth) -and (Get-Item -LiteralPath $auth).LastWriteTimeUtc -gt $was) {
            $in = $true
            $p.StandardInput.Close()
            [void]$p.WaitForExit(15000)
            break
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not $p.HasExited) {
        try { $p.Kill() } catch { $null = $_ }
        if (-not $in) { Stop-Step 'no sign-in in 10 minutes: she says how to sign in when you talk to her' }
    }
    $text = ''
    if ($err.Wait(2000)) { $text = ($err.Result -replace "\x1b\[[0-9;]*m", '').Trim() }
    if (-not $in -and $p.ExitCode) { Stop-Step "synaps login exited $($p.ExitCode): $(($text -split "`n" | Where-Object { $_.Trim() } | Select-Object -Last 1))" }
    Good 'signed in: her brain is in'
    Result 'brain: yes'
}

# Her uninstaller, when he says to remove Synaps too (it installed it): the daemon stopped, synaps.exe,
# its installer's record and, once nothing else is in it, ~\.cargo\bin and its Path entry.
function Remove-Synaps {
    $exe = Join-Path $script:CargoBin 'synaps.exe'
    if (-not (Test-Path -LiteralPath $exe)) { Say "no $exe"; Result 'synaps: missing'; return }
    $ErrorActionPreference = 'Continue'
    & $exe daemon stop *> $null
    $ErrorActionPreference = 'Stop'
    for ($i = 0; $i -lt 10 -and (Get-Process synaps -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }); $i++) { Start-Sleep -Milliseconds 500 }
    Get-Process synaps -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
    Remove-Item -Force -LiteralPath $exe
    Good "removed $exe"
    $record = Join-Path $env:LOCALAPPDATA 'synaps'
    if (Test-Path -LiteralPath $record) { Remove-Item -Recurse -Force -LiteralPath $record; Good "removed $record" }
    if ((Test-Path -LiteralPath $script:CargoBin) -and -not (Get-ChildItem -Force -LiteralPath $script:CargoBin)) {
        Remove-Item -Force -LiteralPath $script:CargoBin
        $cargo = Split-Path -Parent $script:CargoBin
        if ((Test-Path -LiteralPath $cargo) -and -not (Get-ChildItem -Force -LiteralPath $cargo)) { Remove-Item -Force -LiteralPath $cargo }
        $envKey = Get-Item -LiteralPath 'registry::HKEY_CURRENT_USER\Environment'
        $path = $envKey.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
        $keep = @($path -split ';' | Where-Object { $_ -and $_.TrimEnd('\') -ne $script:CargoBin.TrimEnd('\') })
        if ($keep.Count -ne @($path -split ';' | Where-Object { $_ }).Count) {
            Set-ItemProperty -LiteralPath 'registry::HKEY_CURRENT_USER\Environment' -Name Path -Value ($keep -join ';') -Type ExpandString
            Good "$($script:CargoBin) is off your user Path"
        }
    }
    $h = if ($env:SYNAPS_BASE_DIR) { $env:SYNAPS_BASE_DIR } else { Join-Path $env:USERPROFILE '.synaps-cli' }
    if (Test-Path -LiteralPath $h) { Say "your Synaps login and sessions stay in $h (delete it if you want them gone)" }
    Result 'synaps: removed'
}

# ---- her pack -----------------------------------------------------------------------------------

# Her --check: the windowed exe writes to her log, not to a console. Ok, its exit code, and its line.
function Test-Pack([string]$Dir) {
    $log = Join-Path $env:LOCALAPPDATA 'synaps-avatar\synaps-avatar.log'
    $before = if (Test-Path -LiteralPath $log) { (Get-Item -LiteralPath $log).Length } else { 0 }
    $p = Start-Process -FilePath $Exe -ArgumentList "--pack `"$Dir`" --check" -Wait -PassThru -WindowStyle Hidden
    $text = ''
    if (Test-Path -LiteralPath $log) {
        $fs = New-Object IO.FileStream($log, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
        try {
            if ($fs.Length -ge $before) { [void]$fs.Seek($before, [IO.SeekOrigin]::Begin) }
            $text = (New-Object IO.StreamReader($fs)).ReadToEnd()
        } finally { $fs.Dispose() }
    }
    $line = [regex]::Match($text, 'pack `[^`]*` ok: \d+ frames[^,]*').Value
    $last = ($text -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last 2) -join ' | '
    return @{ Ok = ($p.ExitCode -eq 0 -and [bool]$line); Code = $p.ExitCode; Line = $line; Last = $last }
}

function Show-Notices {
    $l = Join-Path $Out 'anim\lachy\LICENSE.md'
    if (-not (Test-Path -LiteralPath $l)) { return }
    Say "lachy's desktop art: PolyForm Noncommercial 1.0.0 (https://polyformproject.org/licenses/noncommercial/1.0.0)"
    Select-String -LiteralPath $l -Pattern '^Required Notice:' | ForEach-Object { Say "  $($_.Line)" }
}

# The inputs a pack is made from: one hash over them (the same files in her installer's copy and the repo).
function Get-Fingerprint {
    $tools = Join-Path $script:Root 'tools'
    $parts = @('tools\make-pack', 'tools\desktop-art.json', 'packs\capri\moods.json', 'packs\capri\persona.md') | ForEach-Object { Join-Path $script:Root $_ }
    $pins = @('capricore-*.sha256', 'lachy-desktop-*.sha256')
    if ($LocalArt) { $pins += 'mochi-desktop-*.sha256'; $parts += Join-Path $tools 'mochi_hair.py' }
    foreach ($p in $pins) { $parts += @(Get-ChildItem -LiteralPath $tools -Filter $p | ForEach-Object { $_.FullName }) }
    $s = (($parts | Sort-Object | ForEach-Object { if (Test-Path -LiteralPath $_) { "$(Split-Path -Leaf $_)=$(Get-Sha256 $_)" } else { "$(Split-Path -Leaf $_)=none" } }) -join ';')
    $s += ";$(Get-Key 'capricore_commit');$(Get-Key 'pip_pack');$(Get-Key 'python_embed');local-art=$([int][bool]$LocalArt)"
    $sha = [Security.Cryptography.SHA256]::Create()
    (-join ($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($s)) | ForEach-Object { $_.ToString('x2') })).Substring(0, 16)
}

# Python for make-pack, in the cache: CPython's embeddable package (no Python of his needed or
# touched), pip from its wheel, and pip_pack (+ pip_local_art) into its site-packages.
function Get-Python {
    $v = Get-Key 'python_embed'
    $tag = 'python' + (($v -split '\.')[0..1] -join '')
    $dir = Join-Path $Cache 'python'
    $py = Join-Path $dir 'python.exe'
    $want = @((Get-Key 'pip_pack') -split '\s+' | Where-Object { $_ })
    if ($LocalArt -and $script:M['pip_local_art']) { $want += @($script:M['pip_local_art'] -split '\s+' | Where-Object { $_ }) }
    $stampText = "$v $(Get-Key 'python_embed_sha256') $(Get-Key 'pip_wheel_sha256') $($want -join ' ')"
    $stamp = Join-Path $dir '.capri-pack'
    if ((Test-Path -LiteralPath $py) -and (Test-Path -LiteralPath $stamp) -and ((Get-Content -Raw -LiteralPath $stamp).Trim() -eq $stampText)) {
        Good "Python $v, $($want -join ' ') (cached)"
        return $py
    }
    if (Test-Path -LiteralPath $dir) { Remove-Item -Recurse -Force -LiteralPath $dir }
    $zip = Join-Path $Cache "python-$v-embed-amd64.zip"
    Get-Url "https://www.python.org/ftp/python/$v/python-$v-embed-amd64.zip" $zip "Python $v"
    Test-Pin $zip (Get-Key 'python_embed_sha256') "python-$v-embed-amd64.zip"
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($zip, $dir)
    Remove-Item -Force -LiteralPath $zip
    # Its search path: the stdlib zip, its own folder, its packages (no site: his user packages stay out).
    Set-Content -Encoding Ascii -LiteralPath (Join-Path $dir "$tag._pth") -Value @("$tag.zip", '.', 'site-packages')
    $wheel = Get-Key 'pip_wheel'
    $whl = Join-Path $Cache (Split-Path -Leaf ([Uri]$wheel).AbsolutePath)
    Get-Url $wheel $whl 'pip'
    Test-Pin $whl (Get-Key 'pip_wheel_sha256') (Split-Path -Leaf $whl)
    Say "pip install $($want -join ' ')"
    $rc = Invoke-Logged $py (@('-X', 'utf8', "$whl\pip", 'install', '-q', '--disable-pip-version-check', '--no-cache-dir', '--no-warn-script-location',
            '--only-binary', ':all:', '--target', (Join-Path $dir 'site-packages')) + $want)
    Remove-Item -Force -LiteralPath $whl
    if ($rc) {
        $l = @($script:Last | Select-Object -Last 1)
        Stop-Step "pip couldn't install $($want -join ' ') from PyPI (exit $($rc): $l). Is this PC offline?"
    }
    Set-Content -Encoding Ascii -LiteralPath $stamp -Value $stampText
    Good "Python $v, $($want -join ' ')"
    return $py
}

# CapriCore at the pinned commit, unpacked in the cache (a half-unpacked one is never used).
function Get-CapriCore {
    $commit = Get-Key 'capricore_commit'
    $short = $commit.Substring(0, 7)
    $cc = Join-Path $Cache "CapriCore-$short"
    if (Test-Path -LiteralPath (Join-Path $cc 'docs\character-assets.md')) { Good "CapriCore $short (cached)"; return $cc }
    $tgz = Join-Path $Cache "CapriCore-$short.tar.gz"
    Get-Url "https://codeload.github.com/$(Get-Key 'capricore_repo')/tar.gz/$commit" $tgz "CapriCore $short"
    $part = "$cc.part"
    if (Test-Path -LiteralPath $part) { Remove-Item -Recurse -Force -LiteralPath $part }
    New-Item -ItemType Directory -Force $part | Out-Null
    Say "unpacking CapriCore $short"
    $rc = Invoke-Logged (Join-Path $env:SystemRoot 'System32\tar.exe') @('-xzf', $tgz, '-C', $part, '--strip-components=1')
    Remove-Item -Force -LiteralPath $tgz
    if ($rc) { Remove-Item -Recurse -Force -LiteralPath $part; Stop-Step "CapriCore's download didn't unpack (tar exit $rc): try again" }
    if (Test-Path -LiteralPath $cc) { Remove-Item -Recurse -Force -LiteralPath $cc }
    Move-Item -LiteralPath $part $cc
    return $cc
}

# NEW into place, marked as these steps' own: the pack there is replaced whole if it's theirs (or
# -Ours: install.ps1 recorded it), else moved aside, kept. Source: what it was made from.
function Set-Pack([string]$New, [string]$Fingerprint, [string]$Source) {
    Set-Content -Encoding Ascii -LiteralPath (Join-Path $New $script:Marker) -Value @(
        "# capri-syn's install steps (packaging/windows/steps.ps1) put this pack here: the next one replaces it",
        '# whole, and her uninstaller removes it. A pack without this file is yours: moved aside, never removed.',
        "source=$Source", "fingerprint=$Fingerprint")
    $old = "$Out.old"
    if (Test-Path -LiteralPath $Out) {
        if (-not (Get-ChildItem -Force -LiteralPath $Out)) { Remove-Item -Force -LiteralPath $Out }
        elseif ($Ours -or (Test-Path -LiteralPath (Join-Path $Out $script:Marker))) {
            if (Test-Path -LiteralPath $old) { Remove-Item -Recurse -Force -LiteralPath $old }
            Move-Item -LiteralPath $Out $old
        } else {
            $aside = "$Out.pre-install-$(Get-Date -Format yyyyMMdd-HHmmss)"
            Warn "$Out is a pack these steps didn't make (yours): it goes to $aside, and stays"
            Move-Item -LiteralPath $Out $aside
        }
    }
    Move-Item -LiteralPath $New $Out
    if (Test-Path -LiteralPath $old) { Remove-Item -Recurse -Force -LiteralPath $old }
}

function Build-Pack {
    if (-not $Exe) { $c = Join-Path (Split-Path -Parent $script:Root) 'synaps-avatar.exe'; if (Test-Path -LiteralPath $c) { $script:Exe = $c } }
    if ($Exe -and -not (Test-Path -LiteralPath $Exe)) { $script:Exe = '' }
    if ($DryRun) {
        Say "`$ make-pack CapriCore@$((Get-Key 'capricore_commit').Substring(0, 7)) $Out --lachy auto$(if ($LocalArt) { ' --mochi auto' }), in CPython $(Get-Key 'python_embed') (embeddable) with $(Get-Key 'pip_pack')"
        Result 'pack: dry-run'
        return
    }
    $fp = Get-Fingerprint
    $mark = Join-Path $Out $script:Marker
    if ((Test-Path -LiteralPath (Join-Path $Out 'sprites.json')) -and (Test-Path -LiteralPath $mark) -and (Select-String -LiteralPath $mark -SimpleMatch "fingerprint=$fp" -Quiet)) {
        $ok = $true
        if ($Exe) { $c = Test-Pack $Out; $ok = $c.Ok; if ($ok) { Good "$($c.Line) (--check)" } }
        if ($ok) { Good "up to date ($Out)"; Show-Notices; Result 'pack: ok'; return }
    }
    New-Item -ItemType Directory -Force $Cache | Out-Null
    $py = Get-Python
    $cc = Get-CapriCore
    $new = "$Out.new"
    if (Test-Path -LiteralPath $new) { Remove-Item -Recurse -Force -LiteralPath $new }
    New-Item -ItemType Directory -Force (Split-Path -Parent $Out) | Out-Null
    $side = @('--lachy', 'auto')
    if ($LocalArt) { $side += @('--mochi', 'auto') }
    Say "building her pack: CapriCore $((Get-Key 'capricore_commit').Substring(0, 7)) and lachy's desktop art, every file checked against its pin"
    # make-pack's download cache (lachy's files) in the cache too.
    $env:XDG_CACHE_HOME = Join-Path $Cache 'xdg'
    try { $rc = Invoke-Logged $py (@('-X', 'utf8', '-u', (Join-Path $script:Root 'tools\make-pack'), $cc, $new) + $side) }
    finally { Remove-Item Env:\XDG_CACHE_HOME -ErrorAction SilentlyContinue }
    if ($rc) {
        Remove-Item -Recurse -Force -LiteralPath $new -ErrorAction SilentlyContinue
        $l = @($script:Last | Where-Object { $_ -like 'make-pack:*' } | Select-Object -Last 1)
        $l = "$l"
        if ($l -match 'is not CapriCore') {
            Remove-Item -Recurse -Force -LiteralPath $cc
            Stop-Step "CapriCore's files don't match their pins (tools/capricore-*.sha256), so none of it was used. That download is deleted: try again"
        }
        if ($l -match 'cannot download|no checksum-matching pose-sleep') { Stop-Step "couldn't download lachy's desktop art or her sleep frame from GitHub. Is this PC offline? ($l)" }
        if ($l -match 'does not match|is not lachydotmcg') { Stop-Step "a file of lachy's desktop art doesn't match its pin (tools/lachy-desktop-*.sha256), so none of it was used ($l)" }
        Stop-Step "make-pack couldn't build her pack (exit $rc): $l"
    }
    if ($Exe) {
        $c = Test-Pack $new
        if (-not $c.Ok) {
            Remove-Item -Recurse -Force -LiteralPath $new
            Stop-Step "the pack it built doesn't pass her --check (exit $($c.Code)): $($c.Last)"
        }
        Good "$($c.Line) (--check)"
    }
    Set-Pack $new $fp 'download'
    Good "her pack: $Out"
    Show-Notices
    if (-not $KeepCache) {
        foreach ($d in @((Join-Path $Cache 'python'), $cc, (Join-Path $Cache 'xdg'))) { if (Test-Path -LiteralPath $d) { Remove-Item -Recurse -Force -LiteralPath $d } }
        if ((Test-Path -LiteralPath $Cache) -and -not (Get-ChildItem -Force -LiteralPath $Cache)) { Remove-Item -Force -LiteralPath $Cache }
    }
    Result 'pack: ok'
}

# Her installer's "Use a pack folder I already have": that folder copied into place.
function Copy-Pack {
    if (-not (Test-Path -LiteralPath $From -PathType Container)) { Stop-Step "there is no folder $From" }
    $src = (Resolve-Path -LiteralPath $From).Path.TrimEnd('\')
    if (-not (Test-Path -LiteralPath (Join-Path $src 'sprites.json'))) { Stop-Step "$From has no sprites.json: it isn't a pack" }
    if ([string]::Equals($src, $Out.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)) { Good "your pack is in place already ($Out)"; Result 'pack: ok'; return }
    if ($DryRun) { Say "`$ copy $src $Out"; Result 'pack: dry-run'; return }
    $new = "$Out.new"
    if (Test-Path -LiteralPath $new) { Remove-Item -Recurse -Force -LiteralPath $new }
    New-Item -ItemType Directory -Force (Split-Path -Parent $Out) | Out-Null
    Say "copying your pack from $src"
    try { Copy-Item -Recurse -LiteralPath $src -Destination $new }
    catch {
        Remove-Item -Recurse -Force -LiteralPath $new -ErrorAction SilentlyContinue
        Stop-Step "couldn't copy $($src): $($_.Exception.Message)"
    }
    Set-Pack $new 'copy' $src
    Good "her pack: $Out (a copy of $src)"
    Show-Notices
    Result 'pack: ok'
}

# ---- her voice ----------------------------------------------------------------------------------

# The microphones: Windows' audio endpoints for capture (MMDEVAPI {0.0.1...}) that work, by the
# names Sound settings shows (the sidecar finds a device by that name).
function Show-Mics {
    $eps = @(Get-CimInstance -ClassName Win32_PnPEntity -Filter "PNPClass = 'AudioEndpoint'" -ErrorAction SilentlyContinue |
            Where-Object { $_.PNPDeviceID -like 'SWD\MMDEVAPI\{0.0.1.*' -and $_.Status -eq 'OK' -and $_.Name } | Sort-Object Name)
    foreach ($e in $eps) { Result "mic: $($e.Name)" }
    if (-not $eps) { Say 'no microphone found' }
    Result "mics: $($eps.Count)"
}

# Where her voice installer comes from: Kind file (-VoiceFrom a path), url (-VoiceFrom a URL), release
# (the latest release's asset), none (no release has one yet) or offline; Where; Sum, the SHA-256
# from the .sha256 beside it ('' when there's none: a file of his, used as he gave it).
function Find-Voice {
    if ($VoiceFrom -match '^https?://') {
        $sum = ''
        try { $sum = ((Get-Text "$VoiceFrom.sha256") -split '\s+')[0] } catch { $sum = '' }
        return @{ Kind = 'url'; Where = $VoiceFrom; Sum = $sum }
    }
    if ($VoiceFrom) {
        if (-not (Test-Path -LiteralPath $VoiceFrom -PathType Leaf)) { return @{ Kind = 'none'; Where = "(no file $VoiceFrom)"; Sum = '' } }
        $f = (Resolve-Path -LiteralPath $VoiceFrom).Path
        $sum = if (Test-Path -LiteralPath "$f.sha256") { ((Get-Content -Raw -LiteralPath "$f.sha256") -split '\s+')[0] } else { '' }
        return @{ Kind = 'file'; Where = $f; Sum = $sum }
    }
    $url = "https://github.com/$(Get-Key 'repo')/releases/latest/download/$(Get-Key 'asset_windows_voice')"
    try { $sum = ((Get-Text "$url.sha256") -split '\s+')[0] } catch {
        $w = Get-NetWhy $_.Exception
        if ($w.Offline) { return @{ Kind = 'offline'; Where = $w.Text; Sum = '' } }
        return @{ Kind = 'none'; Where = '(no release has it yet)'; Sum = '' }
    }
    if ($sum -notmatch '^[0-9a-f]{64}$') { return @{ Kind = 'none'; Where = '(no release has it yet)'; Sum = '' } }
    return @{ Kind = 'release'; Where = $url; Sum = $sum }
}

# Her voice. The component: its installer (Find-Voice), sha256 checked, run unattended, into voice\
# beside her; or -Bundle, a bundle install.ps1 built. Then the models (voice-models.ps1, each file
# pinned), then his choices into settings.toml through her own writer (synaps-avatar --set:
# [voice] enabled, wake_word, speak_unprompted, mic). Never her voice itself (voice, speed, pace, flow):
# she keeps her built-in default, so a new voice is a change to her alone.
function Install-Voice {
    if (-not $Exe) { $c = Join-Path (Split-Path -Parent $script:Root) 'synaps-avatar.exe'; if (Test-Path -LiteralPath $c) { $script:Exe = $c } }
    $voiceKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7E0CA8F-064C-4B7F-939A-41681146411C}_is1'
    $dir = $Bundle
    if (-not $dir) {
        $v = Find-Voice
        switch ($v.Kind) {
            'offline' { Stop-Step "couldn't reach github.com for her voice ($($v.Where)). Is this PC offline?" }
            'none' { Stop-Step "her voice comes with her first release: there's none on GitHub yet $($v.Where)" }
        }
        $what = Split-Path -Leaf ([string]$v.Where)
        $argLine = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /MERGETASKS="!models"'
        if ($DryRun) { Say "`$ $what ($($v.Kind)) $argLine, then voice-models.ps1, then synaps-avatar --set voice.enabled=true ..."; Result 'voice: dry-run'; return }
        if ($v.Kind -eq 'file') { $setup = $v.Where }
        else {
            $setup = Join-Path $script:Tmp $what
            Get-Url $v.Where $setup 'her voice'
        }
        if ($v.Sum -match '^[0-9a-f]{64}$') { Test-Pin $setup $v.Sum $what }
        else { Warn "no .sha256 beside $($v.Where): used as you gave it" }
        Say 'installing her voice (the speech bundle beside her)'
        $p = Start-Process -FilePath $setup -ArgumentList $argLine -Wait -PassThru
        if ($p.ExitCode -ne 0) { Stop-Step "her voice installer stopped (exit $($p.ExitCode))" }
        $dir = (Get-ItemProperty -Path $voiceKey -ErrorAction SilentlyContinue).InstallLocation
        if (-not $dir) { Stop-Step 'her voice installer ran, but left no record of where it went' }
        $dir = $dir.TrimEnd('\')
        Good "her voice: $dir"
    } elseif ($DryRun) { Say "`$ voice-models.ps1 ($dir), then synaps-avatar --set voice.enabled=true ..."; Result 'voice: dry-run'; return }
    $models = Join-Path $dir 'voice-models.ps1'
    if (-not (Test-Path -LiteralPath $models)) { Stop-Step "there's no voice-models.ps1 in $dir" }
    Say 'her speech models (~375 MB, each SHA-256 pinned)'
    $ps = (Get-Process -Id $PID).Path
    $rc = Invoke-Logged $ps @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $models)
    if ($rc) {
        $l = @($script:Last | Where-Object { $_ -like 'FAIL*' } | Select-Object -First 1)
        if ("$l" -match 'curl exit (6|7|28|35)\b') { Stop-Step "couldn't download her speech models ($l). Is this PC offline?" }
        Stop-Step "her speech models didn't all come: $(if ($l) { $l } else { "exit $rc" }) (what came is kept: the retry goes on from there)"
    }
    Result "voice: installed $dir"
    # His choices, by her own settings writer: the rest of settings.toml stays as it was.
    if (-not $Exe) { Stop-Step "her voice is installed, but there's no synaps-avatar.exe here to save your choices with" }
    $set = @("voice.enabled=true", "voice.wake_word=$(if ($WakeWord -eq 'on') { 'true' } else { 'false' })",
        "voice.speak_unprompted=$(if ($ReadAloud -eq 'on') { 'true' } else { 'false' })", "voice.mic=$Mic")
    $argv = ($set | ForEach-Object { "--set `"$_`"" }) -join ' '
    $log = Join-Path $env:LOCALAPPDATA 'synaps-avatar\synaps-avatar.log'
    $before = if (Test-Path -LiteralPath $log) { (Get-Item -LiteralPath $log).Length } else { 0 }
    $p = Start-Process -FilePath $Exe -ArgumentList $argv -Wait -PassThru -WindowStyle Hidden
    if ($p.ExitCode -ne 0) {
        $said = ''
        if (Test-Path -LiteralPath $log) {
            $fs = New-Object IO.FileStream($log, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
            try { if ($fs.Length -ge $before) { [void]$fs.Seek($before, [IO.SeekOrigin]::Begin) }; $said = (New-Object IO.StreamReader($fs)).ReadToEnd() } finally { $fs.Dispose() }
        }
        $said = (($said -split "`r?`n" | Where-Object { $_ -match 'synaps-avatar: ' } | Select-Object -Last 1) -replace '^.*synaps-avatar: ', '').Trim()
        if ($said -match 'unknown argument `--set`') { $said = "this build of her can't save settings from her installer yet (synaps-avatar --set)" }
        Stop-Step "her voice is installed, but your choices weren't saved: $said"
    }
    Good "saved: talk to her on, `"hey Capri`" $WakeWord, replies read aloud $ReadAloud, microphone $(if ($Mic) { $Mic } else { "Windows' default" })"
    Say 'she starts her voice the next time she starts (or on "settings reload")'
    Result 'voice: ok'
}

# ---- her uninstaller's part ---------------------------------------------------------------------

# What these steps made that her installer's own record doesn't have: her pack when it holds their
# marker (one without it is his, and stays; so do packs moved aside), its half-made .new/.old, their own
# Run\synaps-daemon (not one of his), and her voice (its own uninstaller). Synaps stays (synaps-remove).
function Remove-Hers {
    if (Test-Path -LiteralPath (Join-Path $Out $script:Marker)) { Remove-Item -Recurse -Force -LiteralPath $Out; Good "removed her pack ($Out)" }
    elseif (Test-Path -LiteralPath $Out) { Say "kept: $Out (a pack these steps didn't make: yours)" }
    foreach ($d in @("$Out.new", "$Out.old")) { if (Test-Path -LiteralPath $d) { Remove-Item -Recurse -Force -LiteralPath $d } }
    $cur = (Get-ItemProperty -Path $script:RunKey -Name 'synaps-daemon' -ErrorAction SilentlyContinue).'synaps-daemon'
    if ($cur -and (Test-OurRunValue $cur)) { Remove-ItemProperty -Path $script:RunKey -Name 'synaps-daemon'; Good 'removed Run\synaps-daemon (Synaps stays; its daemon runs until you sign out)' }
    $voice = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{B7E0CA8F-064C-4B7F-939A-41681146411C}_is1'
    $u = (Get-ItemProperty -Path $voice -ErrorAction SilentlyContinue).UninstallString
    if ($u) {
        Say 'removing her voice (its own uninstaller)'
        $p = Start-Process -FilePath $u.Trim('"') -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' -Wait -PassThru
        # Inno's uninstaller hands off to a copy of itself: wait for that one too.
        for ($i = 0; $i -lt 120 -and (Test-Path $voice); $i++) { Start-Sleep -Milliseconds 500 }
        if ($p.ExitCode) { Warn "her voice's uninstaller exited $($p.ExitCode)" } else { Good 'removed her voice' }
    }
    Result 'remove: ok'
}

Invoke-Steps
