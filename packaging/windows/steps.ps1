# capri-syn's install steps on Windows, the one copy of them: install.ps1 runs them (its steps 1, 3, 4
# and 7), and so does her installer (capri-syn-setup, synaps-avatar.iss) once her binary is in place.
# One or more steps per run, in order, stopping at the first that fails ("steps.ps1 synaps daemon"):
#
#   steps.ps1 synaps-check   is Synaps here (`synaps` on PATH, else ~\.cargo\bin\synaps.exe) and at
#                            least installer/manifest's synaps_min? Reads only.
#   steps.ps1 synaps         the same, then, when it's missing or older: the manifest's pinned release
#                            (synaps_pin, a Synaps she was run against) and its synaps-installer.ps1,
#                            checked against the pinned SHA-256, run unattended. Never the latest
#                            release: a protocol bump there would leave her unable to attach. Its
#                            installer puts ~\.cargo\bin on his user Path.
#   steps.ps1 daemon         the Synaps daemon now (`synaps daemon --detach` unless it runs) and at
#                            sign-in: Run\synaps-daemon through conhost --headless, so no console window
#                            opens (a Run value of his own that starts it is left as it is). A daemon
#                            whose protocol isn't hers (synaps_protocol, exact match), or that doesn't
#                            say, fails the step (`daemon: wrong-protocol`, with how to fix it). -Fresh:
#                            this run installed or upgraded Synaps (her installer and install.ps1 say
#                            so), so a daemon on another protocol is an older one still running.
#   steps.ps1 pack           her art pack (below).
#   steps.ps1 mics           the microphones Windows has (its capture endpoints that work). Reads only.
#   steps.ps1 voice-check    where her voice would come from (-VoiceFrom, else -Release's: her release,
#                            vX.Y.Z), or none yet. Reads only.
#   steps.ps1 voice          her voice: its installer (-VoiceFrom FILE or https URL, else the one of
#                            -Release, the release she came with; never releases/latest: a voice from
#                            a newer release than her could need a newer her; its .sha256 checked),
#                            or -Bundle DIR already in place;
#                            then its models (~375 MB, each pinned); then [voice] enabled, and the
#                            choices given (-Mic NAME or -MicDefault, -WakeWord, -ReadAloud; one not
#                            given stays as he set it), written to settings.toml by her own settings
#                            writer (synaps-avatar --set).
#   steps.ps1 login          `synaps login --provider claude` with no window: his browser opens, and he
#                            signs in there (given 10 minutes).
#   steps.ps1 synaps-remove  Synaps (-Synaps: the synaps.exe her installer installed and recorded): its
#                            daemon stopped, the binaries its installer's receipt lists (synaps.exe,
#                            hidden.exe) beside it, the receipt, and once nothing else is in it the
#                            folder and its Path entry. His login and sessions (~\.synaps-cli) stay.
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
    [string]$Release = '',
    [string]$Bundle = '',
    [string]$Mic = '',
    [switch]$MicDefault,
    [switch]$Fresh,
    [ValidateSet('', 'on', 'off')] [string]$WakeWord = '',
    [ValidateSet('', 'on', 'off')] [string]$ReadAloud = '',
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
            'synaps-check' { Show-Synaps (Find-Synaps); Result "pin: $(Get-Key 'synaps_pin')" }
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
    # The manifest's pin: a Synaps she was run against (protocol synaps_protocol, exact match), its
    # installer's SHA-256 pinned. Never releases/latest: a protocol bump there bricks new installs.
    $repo = Get-Key 'synaps_repo'
    $tag = "v$(Get-Key 'synaps_pin')"
    $sha = Get-Key 'synaps_installer_ps1_sha256'
    if ($s.State -eq 'old') { Say "synaps $($s.Version) at $($s.Path) is older than $(Get-Key 'synaps_min'): installing $tag" }
    else { Say "installing Synaps $tag with its own installer" }
    Result "was: $($s.State)"
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
    $was = $s.State
    $s = Find-Synaps
    if ($s.State -ne 'ok') { Stop-Step "Synaps' installer ran, but $mine says version '$($s.Version)' (she needs $(Get-Key 'synaps_min') or newer)" }
    $script:Fresh = $true
    # An upgrade: a daemon the old Synaps started runs it until it's stopped. Said now (the daemon
    # step stops if it speaks another protocol).
    if ($was -eq 'old' -and (Get-DaemonStatus $mine).Alive) { Warn "a daemon is running, from the Synaps before this one: stop it now (`"$mine`" daemon stop)" }
    Show-Synaps $s
}

# The daemon status. Alive: a daemon runs, whatever it speaks. Up: it also answered this Synaps'
# ping, exit 0. A daemon on another protocol than this synaps.exe's refuses the ping, and Synaps
# (0.11 and 0.12, src/cmd/daemon.rs `status`) then says "daemon: running (not answering)" and exits 1,
# with its protocol from daemon.json: Alive, not Up. So the protocol checks go on Alive; Up is only
# "it answers" (the wait for a daemon just started, and a hung one of hers).
function Get-DaemonStatus([string]$File) {
    $ErrorActionPreference = 'Continue'
    $t = (& $File daemon status 2>&1 | Out-String)
    $alive = $t -match '(?m)^daemon: running'
    $up = ($LASTEXITCODE -eq 0) -and $alive
    $p = if ($t -match 'protocol: (\d+)') { $Matches[1] } else { '?' }
    return @{ Up = $up; Alive = $alive; Protocol = $p; Text = $t.Trim() }
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
        elseif ($st.Alive) { Say "a daemon runs already, but doesn't answer this Synaps: $(($st.Text -split "`n")[0]) (its protocol: below)" }
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
    if (-not $st.Alive) {
        Warn "the daemon isn't answering yet: $(($st.Text -split "`n")[0])"
        Result 'daemon: not-running'
        Result "brain: $(if (Test-Brain) { 'yes' } else { 'no' })"
        return
    }
    # Never "running" unless it's her protocol: she'd start and never attach. A daemon still up from
    # an older Synaps keeps its protocol until it's stopped (`daemon stop` ends any; its sessions
    # are saved).
    $proto = Get-Key 'synaps_protocol'
    $pin = Get-Key 'synaps_pin'
    $stop = "`"$exe`" daemon stop (--force if it won't)"
    if ($st.Protocol -eq '?') {
        Result 'daemon: wrong-protocol'
        Stop-Step "the daemon doesn't say which protocol it speaks (an older Synaps?); she speaks $proto, exactly: stop it, $stop, then run this again"
    }
    # This run put Synaps in (new, or an upgrade): the daemon still up is an older one; installing
    # the pin again won't change it.
    if ($st.Protocol -ne $proto -and $script:Fresh) {
        Result 'daemon: wrong-protocol'
        Stop-Step "synaps $($s.Version) is installed, but the daemon still running is an older one (protocol $($st.Protocol); hers is $proto): stop it, $stop, then run this again"
    }
    if ($st.Protocol -ne $proto) {
        Result 'daemon: wrong-protocol'
        Stop-Step "the daemon speaks protocol $($st.Protocol); she speaks $proto (exact match), so she can't attach. Install Synaps $pin (irm https://github.com/$(Get-Key 'synaps_repo')/releases/download/v$pin/synaps-installer.ps1 | iex), stop the daemon, $stop, and run this again; or use a capri-syn that speaks $($st.Protocol)"
    }
    # Hers, but it didn't answer in 10 s: hung, never "running".
    if (-not $st.Up) {
        Warn "the daemon runs (protocol $($st.Protocol), hers) but doesn't answer: stop it, $stop, then start it again, `"$exe`" daemon --detach"
        Result 'daemon: not-running'
        Result "brain: $(if (Test-Brain) { 'yes' } else { 'no' })"
        return
    }
    Good "daemon running (protocol $($st.Protocol)): at sign-in too"
    Result "daemon: running $($st.Protocol)"
    Result "brain: $(if (Test-Brain) { 'yes' } else { 'no' })"
}

# A sign-in page these steps open in his browser: https on claude.ai or anthropic.com (or a name
# under them), nothing else, whatever synaps prints.
function Test-SignInUrl([string]$Url) {
    $u = $null
    if (-not [Uri]::TryCreate($Url, [UriKind]::Absolute, [ref]$u)) { return $false }
    if ($u.Scheme -ne 'https' -or $u.UserInfo -or -not $u.IsDefaultPort) { return $false }
    $h = $u.IdnHost.ToLowerInvariant()
    return [bool]($h -match '^(([a-z0-9-]+\.)*)(claude\.ai|anthropic\.com)$')
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
    $null = $p.StandardOutput.ReadToEndAsync()
    Say 'your browser opens: sign in to Claude there (10 minutes)'
    $t0 = Get-Date
    $in = $false
    $said = New-Object Collections.Generic.List[string]
    $state = @{ Line = $p.StandardError.ReadLineAsync(); Opened = $false }
    # Its stderr, what's there so far. Synaps 0.12 opens the page with `cmd /c start URL`, and cmd
    # cuts the URL at its first & (no client_id: claude.ai can't sign him in). The whole URL is on
    # its stderr: opened here, by the shell, which leaves it whole.
    $read = {
        while ($state.Line -and $state.Line.IsCompleted) {
            $r = $state.Line.Result
            if ($null -eq $r) { $state.Line = $null; break }
            $state.Line = $p.StandardError.ReadLineAsync()
            $l = ($r -replace "\x1b\[[0-9;]*m", '').Trim()
            if ($l) { $said.Add($l) }
            if (-not $state.Opened -and $l -match '^https://\S+\?\S*&\S+$') {
                $state.Opened = $true
                if (-not (Test-SignInUrl $l)) { Warn "not opened (not a Claude or Anthropic sign-in page): $l" }
                elseif ($env:CAPRI_NO_BROWSER) { Say "would open: $l" }
                else { try { Start-Process $l; Good 'opened the sign-in page in your browser' } catch { Warn "your browser didn't open: go to $l" } }
            }
        }
    }
    while (-not $p.HasExited -and ((Get-Date) - $t0).TotalMinutes -lt 10) {
        & $read
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
    for ($i = 0; $i -lt 30 -and $state.Line; $i++) { & $read; if ($state.Line) { Start-Sleep -Milliseconds 100 } }
    if (-not $in -and $p.ExitCode) { Stop-Step "synaps login exited $($p.ExitCode): $($said | Where-Object { $_ } | Select-Object -Last 1)" }
    Good 'signed in: her brain is in'
    Result 'brain: yes'
}

# Her uninstaller, when he says to remove Synaps too (it installed it): -Synaps, the synaps.exe it
# installed and recorded (never one found now: CARGO_HOME may point elsewhere since). Its daemon stopped;
# the binaries its installer's receipt lists (synaps.exe, hidden.exe), from beside it; the receipt (only
# that file: its folder goes once empty); and once nothing else is in it, the folder and its Path entry.
function Remove-Synaps {
    $exe = if ($Synaps) { $Synaps } else { Join-Path $script:CargoBin 'synaps.exe' }
    if ((Split-Path -Leaf $exe) -ne 'synaps.exe') { Stop-Step "not a synaps.exe: $exe" }
    if (-not (Test-Path -LiteralPath $exe)) { Say "no $exe"; Result 'synaps: missing'; return }
    $bin = Split-Path -Parent $exe
    $ErrorActionPreference = 'Continue'
    & $exe daemon stop *> $null
    $ErrorActionPreference = 'Stop'
    # The receipt: where synaps-installer.ps1 writes it ($XDG_CONFIG_HOME\synaps, else %LOCALAPPDATA%\synaps).
    $rh = if ($env:XDG_CONFIG_HOME) { Join-Path $env:XDG_CONFIG_HOME 'synaps' } else { Join-Path $env:LOCALAPPDATA 'synaps' }
    $rf = Join-Path $rh 'synaps-receipt.json'
    $bins = @('synaps.exe')
    if (Test-Path -LiteralPath $rf) {
        try { $r = Get-Content -Raw -LiteralPath $rf | ConvertFrom-Json; if ($r.binaries) { $bins = @($r.binaries) + @('synaps.exe') | Select-Object -Unique } }
        catch { Warn "its installer's receipt ($rf) doesn't read: synaps.exe only" }
    }
    foreach ($b in $bins) {
        if ("$b" -notmatch '^[\w.-]+\.exe$') { continue }
        $f = Join-Path $bin $b
        for ($i = 0; $i -lt 10 -and (Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $f }); $i++) { Start-Sleep -Milliseconds 500 }
        Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $f } | Stop-Process -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $f) {
            Start-Sleep -Milliseconds 300
            Remove-Item -Force -LiteralPath $f
            Good "removed $f"
        }
    }
    if (Test-Path -LiteralPath $rf) { Remove-Item -Force -LiteralPath $rf; Good "removed its installer's receipt ($rf)" }
    if ((Test-Path -LiteralPath $rh) -and -not (Get-ChildItem -Force -LiteralPath $rh)) { Remove-Item -Force -LiteralPath $rh }
    if ((Test-Path -LiteralPath $bin) -and -not (Get-ChildItem -Force -LiteralPath $bin)) {
        Remove-Item -Force -LiteralPath $bin
        Good "removed $bin (empty)"
        if ((Split-Path -Leaf $bin) -eq 'bin') {
            $cargo = Split-Path -Parent $bin
            if ((Test-Path -LiteralPath $cargo) -and -not (Get-ChildItem -Force -LiteralPath $cargo)) { Remove-Item -Force -LiteralPath $cargo }
        }
        $envKey = Get-Item -LiteralPath 'registry::HKEY_CURRENT_USER\Environment'
        $path = $envKey.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
        $keep = @($path -split ';' | Where-Object { $_ -and [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -ne $bin.TrimEnd('\') })
        if ($keep.Count -ne @($path -split ';' | Where-Object { $_ }).Count) {
            Set-ItemProperty -LiteralPath 'registry::HKEY_CURRENT_USER\Environment' -Name Path -Value ($keep -join ';') -Type ExpandString
            Good "$bin is off your user Path"
        }
    } elseif (Test-Path -LiteralPath $bin) { Say "kept: $bin (other programs' files are in it)" }
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
    $hashes = @((Get-Key 'pip_pack_sha256') -split '\s+' | Where-Object { $_ -match '^[0-9a-f]{64}$' })
    if (-not $hashes) { Stop-Step 'installer/manifest: no pip_pack_sha256' }
    # --with-local-art's (ranges, LOCAL PERSONAL USE ONLY): no hashes to pin, so a pip run of their own.
    $extra = @(if ($LocalArt -and $script:M['pip_local_art']) { $script:M['pip_local_art'] -split '\s+' | Where-Object { $_ } })
    $stampText = "$v $(Get-Key 'python_embed_sha256') $(Get-Key 'pip_wheel_sha256') $($want -join ' ') $($hashes -join ' ') $($extra -join ' ')"
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
    # pip_pack by its pinned wheels' hashes (--require-hashes: a wheel PyPI serves that isn't one of
    # them is refused, as is any dependency it would pull in).
    $req = Join-Path $Cache 'pip-pack.txt'
    Set-Content -Encoding Ascii -LiteralPath $req -Value @($want | ForEach-Object { "$_ " + (($hashes | ForEach-Object { "--hash=sha256:$_" }) -join ' ') })
    $pip = @('-X', 'utf8', "$whl\pip", 'install', '-q', '--disable-pip-version-check', '--no-cache-dir', '--no-warn-script-location',
        '--only-binary', ':all:', '--target', (Join-Path $dir 'site-packages'))
    Say "pip install $($want -join ' ') (hashes pinned)"
    $rc = Invoke-Logged $py ($pip + @('--require-hashes', '-r', $req))
    Remove-Item -Force -LiteralPath $req
    if (-not $rc -and $extra) {
        Say "pip install $($extra -join ' ') (--with-local-art)"
        $rc = Invoke-Logged $py ($pip + @('--upgrade') + $extra)
    }
    Remove-Item -Force -LiteralPath $whl
    if ($rc) {
        $l = @($script:Last | Select-Object -Last 1)
        if ("$l" -match 'hash|HASH') { Stop-Step "pip refused $($want -join ' '): its wheel isn't the one pinned ($l)" }
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
    # The one moved out of the way: put back if the new one can't go in.
    $back = $null
    if (Test-Path -LiteralPath $Out) {
        if (-not (Get-ChildItem -Force -LiteralPath $Out)) { Remove-Item -Force -LiteralPath $Out }
        elseif ($Ours -or (Test-Path -LiteralPath (Join-Path $Out $script:Marker))) {
            if (Test-Path -LiteralPath $old) { Remove-Item -Recurse -Force -LiteralPath $old }
            Move-Item -LiteralPath $Out $old
            $back = $old
        } else {
            $aside = "$Out.pre-install-$(Get-Date -Format yyyyMMdd-HHmmss)"
            Warn "$Out is a pack these steps didn't make (yours): it goes to $aside, and stays"
            Move-Item -LiteralPath $Out $aside
            $back = $aside
        }
    }
    try { Move-Item -LiteralPath $New $Out }
    catch {
        $why = $_.Exception.Message
        if ($back -and -not (Test-Path -LiteralPath $Out)) {
            try { Move-Item -LiteralPath $back $Out; Say "$Out is as it was" }
            catch { Warn "and the pack that was there couldn't be put back: it is in $back" }
        }
        Stop-Step "her new pack couldn't go into $Out ($why)"
    }
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

# Where her voice installer comes from: Kind file (-VoiceFrom a path), url (-VoiceFrom an https URL),
# release (the latest release's asset), none (no release has one yet) or offline; Where; Sum, the
# SHA-256 from the .sha256 beside it ('' when there's none: a file of his, used as he gave it).
function Find-Voice {
    if ($VoiceFrom -match '^[a-z][a-z0-9+.-]*://' -and $VoiceFrom -notmatch '^https://') {
        return @{ Kind = 'none'; Where = "(https only, not $VoiceFrom)"; Sum = '' }
    }
    if ($VoiceFrom -match '^https://') {
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
    # Her release's own voice installer: the tag of the setup she came with (her installer passes its
    # version, install.ps1 the one in her Apps entry). Never releases/latest.
    if ($Release -notmatch '^v\d+\.\d+\.\d+([-+][0-9A-Za-z.-]+)?$') { return @{ Kind = 'none'; Where = "(no release of hers given$(if ($Release) { ": $Release" }))"; Sum = '' } }
    $url = "https://github.com/$(Get-Key 'repo')/releases/download/$Release/$(Get-Key 'asset_windows_voice')"
    try { $sum = ((Get-Text "$url.sha256") -split '\s+')[0] } catch {
        $w = Get-NetWhy $_.Exception
        # Windows PowerShell can read github.com's 404 (no release) as a closed connection: offline
        # only when the repo itself doesn't answer either.
        if ($w.Offline) {
            try { $null = Invoke-WebRequest -UseBasicParsing -Method Head -Uri "https://github.com/$(Get-Key 'repo')" -Headers (Get-Headers) -TimeoutSec 30 }
            catch { return @{ Kind = 'offline'; Where = $w.Text; Sum = '' } }
        }
        return @{ Kind = 'none'; Where = "(release $Release doesn't have it yet)"; Sum = '' }
    }
    if ($sum -notmatch '^[0-9a-f]{64}$') { return @{ Kind = 'none'; Where = "(release $Release doesn't have it yet)"; Sum = '' } }
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
    # His choices, by her own settings writer: the rest of settings.toml stays as it was, and so does
    # any choice not given here (install.ps1 gives none: a re-run keeps his mic, wake word, read-aloud).
    if (-not $Exe) { Stop-Step "her voice is installed, but there's no synaps-avatar.exe here to save your choices with" }
    $set = @('voice.enabled=true')
    if ($WakeWord) { $set += "voice.wake_word=$(if ($WakeWord -eq 'on') { 'true' } else { 'false' })" }
    if ($ReadAloud) { $set += "voice.speak_unprompted=$(if ($ReadAloud -eq 'on') { 'true' } else { 'false' })" }
    if ($Mic) { $set += "voice.mic=$Mic" } elseif ($MicDefault) { $set += 'voice.mic=' }
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
        # synaps-avatar --set's exit codes: 2 a bad --set, 3 settings.toml refused (a typo in it, or
        # unreadable: never written over), 4 not saved.
        $settings = Join-Path $env:APPDATA 'synaps-avatar\settings.toml'
        if (-not $said) {
            $said = switch ($p.ExitCode) {
                3 { "your settings file ($settings) has a typo, and she won't write over it: fix it, then run this again" }
                4 { "she couldn't write $settings" }
                default { "synaps-avatar --set exited $($p.ExitCode)" }
            }
        } elseif ($p.ExitCode -eq 3) { $said = "$said Fix it ($settings), then run this again." }
        Stop-Step "her voice is installed, but your choices weren't saved: $said"
    }
    $mine = @('talk to her on')
    if ($WakeWord) { $mine += "`"hey Capri`" $WakeWord" }
    if ($ReadAloud) { $mine += "replies read aloud $ReadAloud" }
    if ($Mic) { $mine += "microphone $Mic" } elseif ($MicDefault) { $mine += "microphone: Windows' default" }
    if ($set.Count -eq 1) { $mine += 'the rest as you had it' }
    Good "saved: $($mine -join ', ')"
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
    # Her control socket: she's stopped by now (taskkill), and a killed process leaves the file.
    # Only when nothing answers on it: another of her (a dev build) may be running.
    $sock = Join-Path ([IO.Path]::GetTempPath()) 'synaps-avatar.sock'
    for ($i = 0; $i -lt 20 -and (Get-Process -Name 'synaps-avatar' -ErrorAction SilentlyContinue); $i++) { Start-Sleep -Milliseconds 250 }
    if ((Test-Path -LiteralPath $sock) -and -not (Get-Process -Name 'synaps-avatar' -ErrorAction SilentlyContinue)) {
        try { [IO.File]::Delete($sock); Good "removed her control socket ($sock)" } catch { Warn "her control socket stays: $($_.Exception.Message)" }
    }
    Result 'remove: ok'
}

Invoke-Steps
