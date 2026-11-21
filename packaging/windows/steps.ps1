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
#   steps.ps1 brain-check    her brain: is there one (`brain: yes|no`), the accounts and key services
#                            she can take (`brains: id|login|name|how|why not|hint`, from -Exe's table:
#                            synaps-avatar --providers), and the models on this PC (`local:
#                            server|url|model`: Ollama's, LM Studio's). Reads only.
#   steps.ps1 login          `synaps login --provider` -Provider's (Claude when none), with no window:
#                            his browser opens on the sign-in page, and he signs in there (given 10
#                            minutes). Copilot's page needs a code: a small window shows it (and it's on
#                            the clipboard). The page's URL and the code never go to this step's output.
#   steps.ps1 key            -Provider's API key, from $env:CAPRI_BRAIN_KEY (taken out of the
#                            environment first), to `synaps login --provider` on its stdin: never on a
#                            command line or in a log.
#   steps.ps1 local          -Model on this PC's server (-Url; Ollama's when none): Synaps'
#                            provider.local.url when it isn't that already (its daemon reloaded when it
#                            runs), then her model, local/<model>.
#   steps.ps1 model          her model (-Exe's settings writer): -Model, else -Provider's default (her
#                            own table: synaps-avatar --providers).
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
    [Parameter(Position = 0, Mandatory, ValueFromRemainingArguments)] [ValidateSet('synaps-check', 'synaps', 'daemon', 'pack', 'mics', 'voice-check', 'voice', 'brain-check', 'login', 'key', 'local', 'model', 'synaps-remove', 'remove')] [string[]]$Step,
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
    [string]$Provider = '',
    [string]$Model = '',
    [string]$Url = '',
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
            'brain-check' { Show-Brains }
            'login' { Invoke-Login }
            'key' { Invoke-Key }
            'local' { Set-Local }
            'model' { Set-HerModel }
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

# ---- her brain: the model she thinks with --------------------------------------------------------

# Her brains are her own table (synaps-avatar --providers: src/provider.rs' ALL, in its order, and
# how each is signed in to: browser, code, key), never a copy here; the local server isn't one (Find-
# Local finds its models). Read once (Get-Brains).
$script:BrainList = $null
$script:BrainsWhy = ''
# Synaps 0.12 opens a sign-in page with `cmd /c start URL` (auth/browser.rs), and cmd cuts the URL at
# its first &. Grok's flow never prints its URL (auth/xai.rs), so its page can't open whole on
# Windows: not offered here until it does.
$script:NotHere = @{ 'xai-auth' = "Synaps 0.12 can't open xAI's sign-in page whole on Windows yet" }
# Each account's sign-in page, as the login step opens it again whole: the line after Synaps' "If the
# browser didn't open, visit this URL:" (Gemini's: "visit:"), https on exactly this host and path
# (auth/mod.rs, openai_codex.rs, google_gemini.rs: their AUTHORIZE_URL). '': a device sign-in, whose
# page has no & to cut (Copilot's, Kimi's, Muse's: a code to type, or filled in): nothing to reopen.
$script:Reopen = @{
    'anthropic' = 'claude.ai/oauth/authorize'; 'openai-codex' = 'auth.openai.com/oauth/authorize'
    'google-gemini' = 'accounts.google.com/o/oauth2/v2/auth'
    'github-copilot' = ''; 'kimi-code' = ''; 'muse' = ''
}
# A model's id as it may reach a command line (her --set, the steps' -Model): nothing else is listed.
$script:ModelId = '^[A-Za-z0-9._:/@+-]+\z'
# The model servers she can think with on this PC: Ollama's (Synaps' default) and LM Studio's.
$script:Servers = @(
    @{ Name = 'Ollama'; Url = 'http://localhost:11434/v1'; Probe = 'http://127.0.0.1:11434/v1/models' },
    @{ Name = 'LM Studio'; Url = 'http://localhost:1234/v1'; Probe = 'http://127.0.0.1:1234/v1/models' })

# Her exe: -Exe, else the one beside these steps (her install folder's; in her setup, its
# temporary copy's), else none.
function Resolve-Her {
    if (-not $Exe) { $c = Join-Path (Split-Path -Parent $script:Root) 'synaps-avatar.exe'; if (Test-Path -LiteralPath $c) { $script:Exe = $c } }
    if ($Exe -and -not (Test-Path -LiteralPath $Exe)) { $script:Exe = '' }
}
# Her accounts and key services, from her table (Id, Login, Name, How, Model: its default; Hint:
# shown beside its name, most often none, and none from an older her); none, and BrainsWhy says
# why, when there's no exe of hers or it has no table.
function Get-Brains {
    if ($null -eq $script:BrainList) {
        $script:BrainList = @()
        Resolve-Her
        if (-not $Exe) { $script:BrainsWhy = "there's no synaps-avatar.exe here to list her brains" }
        else {
            $r = Invoke-Her @('--providers')
            foreach ($l in $r.Out) {
                $f = @($l -split "`t")
                if ($f.Count -ge 6 -and $f[2] -in @('account', 'key') -and $f[0] -and $f[1] -and $f[4]) {
                    $script:BrainList += [pscustomobject]@{ Id = $f[0]; Login = $f[1]; Name = $f[4]; How = $f[5]; Model = $f[3]; Hint = $(if ($f.Count -ge 7) { $f[6] } else { '' }) }
                }
            }
            if (-not $script:BrainList.Count) { $script:BrainsWhy = "this build of her can't list her brains (synaps-avatar --providers)" }
        }
    }
    $script:BrainList
}
function Find-Brain([string]$Id) {
    $all = @(Get-Brains)
    if (-not $all.Count) { Stop-Step $script:BrainsWhy }
    $b = $all | Where-Object { $_.Id -eq $Id -or $_.Login -eq $Id } | Select-Object -First 1
    if (-not $b) { Stop-Step "there's no brain called '$Id' (one of: $(($all | ForEach-Object { $_.Id }) -join ', '))" }
    return $b
}
function Get-SynapsHome { if ($env:SYNAPS_BASE_DIR) { $env:SYNAPS_BASE_DIR } else { Join-Path $env:USERPROFILE '.synaps-cli' } }

# The models on this PC's servers (OpenAI's /v1/models; 2 s each, nothing sent but the ask). An
# embedding model isn't one to talk with.
function Find-Local {
    foreach ($s in $script:Servers) {
        try { $r = Invoke-RestMethod -UseBasicParsing -Uri $s.Probe -TimeoutSec 2 -ErrorAction Stop } catch { continue }
        foreach ($m in @($r.data)) {
            $id = "$($m.id)".Trim()
            if ($id -cmatch $script:ModelId -and $id -notmatch 'embed') { [pscustomobject]@{ Server = $s.Name; Url = $s.Url; Model = $id } }
        }
    }
}

function Show-Brains {
    Result "brain: $(if (Test-Brain) { 'yes' } else { 'no' })"
    $all = @(Get-Brains)
    if (-not $all.Count) { Warn "no accounts or key services: $script:BrainsWhy" }
    foreach ($b in $all) {
        # Gemini's sign-in needs its Google client (Synaps' google_gemini.rs): not without it.
        if ($b.Id -eq 'google-gemini' -and -not $env:SYNAPS_GOOGLE_GEMINI_CLIENT_ID) { continue }
        Result "brains: $($b.Id)|$($b.Login)|$($b.Name)|$($b.How)|$($script:NotHere[$b.Id])|$($b.Hint)"
    }
    $n = 0
    foreach ($l in Find-Local) { Result "local: $($l.Server)|$($l.Url)|$($l.Model)"; $n++ }
    if ($n) { Good "$n model(s) on this PC" } else { Say 'no model server on this PC (Ollama, LM Studio)' }
}

# The sign-in page -Id's login opens in his browser, from Synaps' line ($Url): its whole URL
# ([Uri]'s AbsoluteUri), when the line is one word with no \ in it (Windows reads one as /), https
# on exactly -Id's host (no user@, no port) and path ($script:Reopen), no #, with an & that cmd cut;
# else ''. Whatever else synaps prints is never opened. The same rules as her own sign-in
# (src/signin.rs' allowed); a Rust test holds $script:Reopen's pages to it.
function Get-SignInPage([string]$Url, [string]$Id = 'anthropic') {
    $want = "$($script:Reopen[$Id])"
    if (-not $want -or $Url -match '[\s\\#\x00-\x1f\x7f]' -or -not $Url.Contains('&')) { return '' }
    $site, $path = $want.Split('/', 2)
    $path = "/$path"
    # The raw line's own start: https://HOST/ (a user@ or a :port never gets past this).
    if (-not $Url.StartsWith("https://$site/", [StringComparison]::OrdinalIgnoreCase)) { return '' }
    $u = $null
    if (-not [Uri]::TryCreate($Url, [UriKind]::Absolute, [ref]$u)) { return '' }
    if ($u.Scheme -ne 'https' -or $u.UserInfo -or $u.Port -ne 443 -or $u.IdnHost -ne $site -or $u.Authority -ne $site) { return '' }
    if ($u.AbsolutePath -cne $path -or $u.Fragment -or -not $u.Query.Contains('&')) { return '' }
    return $u.AbsoluteUri
}

# Copilot's code, in a small window of its own (and on the clipboard), never in a log or in this
# step's output: Invoke-Login shows it modal (its own message loop) until he's signed in, or closes
# it (Cancel, Esc, its X): then the sign-in is stopped.
function New-CodeWindow([string]$Code, [string]$Name) {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    if (-not ('CapriSteps.Win' -as [type])) {
        Add-Type -Namespace CapriSteps -Name Win -MemberDefinition '[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c); [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);'
    }
    try { [Windows.Forms.Clipboard]::SetText($Code) } catch { $null = $_ }
    $f = New-Object Windows.Forms.Form
    $f.Text = "Sign in to $Name"
    $f.FormBorderStyle = 'FixedDialog'; $f.MaximizeBox = $false; $f.MinimizeBox = $false
    $f.StartPosition = 'CenterScreen'; $f.TopMost = $true; $f.ShowInTaskbar = $true
    $f.AutoScaleMode = 'Dpi'
    $f.ClientSize = New-Object Drawing.Size(400, 176)
    $f.BackColor = [Drawing.Color]::FromArgb(10, 10, 11); $f.ForeColor = [Drawing.Color]::White
    $a = New-Object Windows.Forms.Label
    $a.Text = "Type this code on the sign-in page that opened in your browser. It's on your clipboard too."
    $a.SetBounds(16, 14, 368, 36)
    $c = New-Object Windows.Forms.TextBox
    $c.Text = $Code; $c.ReadOnly = $true; $c.BorderStyle = 'None'; $c.TextAlign = 'Center'
    $c.BackColor = $f.BackColor; $c.ForeColor = [Drawing.Color]::FromArgb(255, 20, 56)
    $c.Font = New-Object Drawing.Font('Consolas', 24, [Drawing.FontStyle]::Bold)
    $c.SetBounds(16, 56, 368, 44)
    $w = New-Object Windows.Forms.Label
    $w.Text = 'This closes once you''re signed in.'
    $w.ForeColor = [Drawing.Color]::Silver
    $w.SetBounds(16, 112, 250, 20)
    $x = New-Object Windows.Forms.Button
    $x.Text = 'Cancel'; $x.FlatStyle = 'System'; $x.DialogResult = 'Cancel'
    $x.SetBounds(296, 136, 88, 28)
    $f.CancelButton = $x
    $f.Controls.AddRange(@($a, $c, $w, $x))
    return $f
}

# The code off his clipboard, when it's still what's there (he may have copied something since).
function Clear-Code([string]$Code) {
    try { if ([Windows.Forms.Clipboard]::GetText() -ceq $Code) { [Windows.Forms.Clipboard]::Clear() } } catch { $null = $_ }
}

# synaps (its path) with these arguments, no window, its stdin, stdout and stderr ours. Windows
# PowerShell's Process.Start writes the console's input encoding's BOM on a child's stdin at once
# (.NET Framework, a UTF-8 console), and Synaps 0.12 trims a key but not a BOM (a broken key,
# stored): so that encoding is made one without, for this process, before it starts.
function Start-Synaps([string]$File, [string]$ArgLine) {
    $bom = { try { [Console]::InputEncoding.GetPreamble().Length } catch { 0 } }
    if (& $bom) {
        try { [Console]::InputEncoding = New-Object Text.UTF8Encoding $false } catch { $null = $_ }
        if (& $bom) { Stop-Step "synaps' input can't be made plain here (this console's encoding: $([Console]::InputEncoding.WebName))" }
    }
    $pi = New-Object Diagnostics.ProcessStartInfo
    $pi.FileName = $File
    $pi.Arguments = $ArgLine
    $pi.UseShellExecute = $false
    $pi.CreateNoWindow = $true
    $pi.RedirectStandardInput = $true
    $pi.RedirectStandardOutput = $true
    $pi.RedirectStandardError = $true
    return [Diagnostics.Process]::Start($pi)
}

# Her installer's sign-in (and install.ps1's): Synaps' login for -Provider (Claude when none), with
# no window. Synaps opens his browser and waits on its localhost callback (or, Copilot, Kimi and Muse,
# on the device code). Its stdin is a pipe held open: at end of input (a closed or null stdin) synaps
# 0.12's login gives up at once ("Manual input channel closed"), and after a sign-in it waits on that
# read; so the pipe is closed once auth.json changes, and then it exits. What it prints is read,
# never passed on: the page's URL and a code stay out of this step's output (and so out of every log).
# Copilot's code window closed: synaps is stopped, and the step fails, saying so.
function Invoke-Login {
    $b = Find-Brain $(if ($Provider) { $Provider } else { 'anthropic' })
    if ($b.How -eq 'key') { Stop-Step "$($b.Name) takes an API key, not a sign-in (steps.ps1 key)" }
    if ($script:NotHere[$b.Id]) { Stop-Step "$($script:NotHere[$b.Id]): pick another brain in her Settings" }
    $s = Find-Synaps
    if ($s.State -eq 'missing') { Stop-Step 'there is no Synaps to sign in' }
    if ($DryRun) { Say "`$ synaps login --provider $($b.Login) (your browser opens)"; Result 'brain: dry-run'; return }
    $auth = Join-Path (Get-SynapsHome) 'auth.json'
    $was = if (Test-Path -LiteralPath $auth) { (Get-Item -LiteralPath $auth).LastWriteTimeUtc } else { [datetime]::MinValue }
    $p = Start-Synaps $s.Path "login --provider $($b.Login)"
    $null = $p.StandardOutput.ReadToEndAsync()
    if ($b.How -eq 'code') { Say "your browser opens: sign in to $($b.Name) there, with the code a small window shows you (10 minutes)" }
    else { Say "your browser opens: sign in to $($b.Name) there (10 minutes)" }
    $t0 = Get-Date
    $said = New-Object Collections.Generic.List[string]
    $state = @{ Line = $p.StandardError.ReadLineAsync(); Next = $false; Opened = $false; Code = $null; In = $false; Done = $false; Raised = $false; Err = $null }
    # Its stderr, what's there so far. The sign-in page (the line after "If the browser didn't open,
    # visit...") is opened here whole, by the shell, when it's -Provider's own (Get-SignInPage).
    # Copilot's code is kept for its window.
    $read = {
        while ($state.Line -and $state.Line.IsCompleted) {
            $r = $state.Line.Result
            if ($null -eq $r) { $state.Line = $null; break }
            $state.Line = $p.StandardError.ReadLineAsync()
            $l = ($r -replace "\x1b\[[0-9;]*m", '').Trim()
            # The page: the very next line, alone (a blank one there: none).
            if ($state.Next) {
                $state.Next = $false
                $state.Opened = $true
                $page = Get-SignInPage $l $b.Id
                if (-not $page) { Warn "not opened: the page Synaps gave isn't $($b.Name)'s sign-in page" }
                elseif ($env:CAPRI_NO_BROWSER) { Add-Content -LiteralPath $env:CAPRI_NO_BROWSER -Value $page; Say 'would open the sign-in page (CAPRI_NO_BROWSER)' }
                else { try { Start-Process $page; Good 'opened the sign-in page in your browser' } catch { Warn "your browser didn't open: run ``synaps login --provider $($b.Login)`` in a terminal" } }
            }
            if (-not $l) { continue }
            $said.Add($l)
            if (-not $state.Opened -and $script:Reopen[$b.Id] -and $l -match "^If the browser didn't open, visit( this URL)?:$") { $state.Next = $true }
            if ($b.How -eq 'code' -and -not $state.Code -and $l -match '^(Enter|Confirm) code:\s*(\S+)$') { $state.Code = $Matches[2] }
        }
    }
    $signed = { (Test-Path -LiteralPath $auth) -and (Get-Item -LiteralPath $auth).LastWriteTimeUtc -gt $was }
    $late = { ((Get-Date) - $t0).TotalMinutes -ge 10 }
    $closed = $false
    # Anything thrown here: synaps is stopped first.
    try {
        while (-not $p.HasExited -and -not (& $late)) {
            & $read
            if (& $signed) { $state.In = $true; break }
            if (-not $state.Code) { Start-Sleep -Milliseconds 250; continue }
            # Copilot's code: its window, until he's signed in (then it closes) or he closes it. Raised
            # once shown: a process started hidden (her installer's finish page) has its first show hidden.
            $f = New-CodeWindow $state.Code $b.Name
            $tick = New-Object Windows.Forms.Timer
            $tick.Interval = 200
            $tick.Add_Tick({
                    try {
                        if (-not $state.Raised) {
                            $state.Raised = $true
                            [void][CapriSteps.Win]::ShowWindow($f.Handle, 5)
                            [void][CapriSteps.Win]::SetForegroundWindow($f.Handle)
                        }
                        & $read
                        if (& $signed) { $state.In = $true }
                        if ($state.In -or $p.HasExited -or (& $late)) { $state.Done = $true; $f.Close() }
                    } catch { $state.Err = "$_"; $state.Done = $true; $f.Close() }
                })
            $tick.Start()
            Good 'the code is in its window, and on your clipboard'
            try { [void]$f.ShowDialog() } finally { $tick.Stop(); $tick.Dispose(); $f.Dispose(); Clear-Code $state.Code }
            if ($state.Err) { throw $state.Err }
            $closed = -not $state.Done
            break
        }
    } catch { try { $p.Kill() } catch { $null = $_ }; throw }
    if (-not $state.In -and -not $p.HasExited) {
        try { $p.Kill() } catch { $null = $_ }
        [void]$p.WaitForExit(5000)
        if ($closed) { Stop-Step "the $($b.Name) sign-in was stopped (its window closed): her Settings > Brain signs in later" }
        Stop-Step 'no sign-in in 10 minutes: she says how to sign in when you talk to her'
    }
    if ($state.In) {
        # Signed in: its stdin closed, it exits (15 s, else it's stopped: the sign-in is kept).
        try { $p.StandardInput.Close() } catch { $null = $_ }
        if (-not $p.WaitForExit(15000)) { try { $p.Kill() } catch { $null = $_ } }
    }
    for ($i = 0; $i -lt 30 -and $state.Line; $i++) { & $read; if ($state.Line) { Start-Sleep -Milliseconds 100 } }
    if (-not $state.In -and $p.ExitCode) {
        # Its own last words, but never a URL or the code.
        $why = $said | Where-Object { $_ -and $_ -notmatch '://' -and -not ($state.Code -and $_.Contains($state.Code)) -and $_ -notmatch 'code:' } | Select-Object -Last 1
        Stop-Step "synaps login exited $($p.ExitCode): $(($why -replace '^[^\w]*Login failed:\s*', ''))"
    }
    Good "signed in to $($b.Name): her brain is in"
    Result 'brain: yes'
}

# -Provider's API key: from $env:CAPRI_BRAIN_KEY, out of the environment before anything starts (so
# synaps doesn't inherit it), then on synaps login's stdin, Synaps' own way for a key from a pipe
# (cmd/login.rs: read_secret_line). Never on a command line, in a log or in this step's output.
function Invoke-Key {
    $key = "$env:CAPRI_BRAIN_KEY".Trim()
    Remove-Item Env:\CAPRI_BRAIN_KEY -ErrorAction SilentlyContinue
    $b = Find-Brain $Provider
    if ($b.How -ne 'key') { Stop-Step "$($b.Name) is signed in to, not given a key (steps.ps1 login)" }
    if ($DryRun) { Say "`$ synaps login --provider $($b.Login) (the key on its stdin)"; Result 'brain: dry-run'; return }
    if (-not $key) { Stop-Step "no $($b.Name) key was given" }
    if ($key -match '\s') { Stop-Step "that $($b.Name) key has a space or a line break in it: paste it again" }
    $s = Find-Synaps
    if ($s.State -eq 'missing') { Stop-Step "there is no Synaps to keep the key" }
    $p = Start-Synaps $s.Path "login --provider $($b.Login)"
    $out = $p.StandardOutput.ReadToEndAsync()
    $err = $p.StandardError.ReadToEndAsync()
    # Its bytes, and a line's end: nothing before them (no BOM), nothing of an encoding's own.
    $bytes = (New-Object Text.UTF8Encoding $false).GetBytes("$key`n")
    $key = $null
    try {
        $p.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
        $p.StandardInput.BaseStream.Flush()
    } catch { $null = $_ } finally {
        [Array]::Clear($bytes, 0, $bytes.Length)
        try { $p.StandardInput.Close() } catch { $null = $_ }
    }
    if (-not $p.WaitForExit(30000)) { try { $p.Kill() } catch { $null = $_ }; Stop-Step "synaps login didn't take the $($b.Name) key in 30 s" }
    $null = $out.Result
    if ($p.ExitCode) {
        $why = (("$($err.Result)" -replace "\x1b\[[0-9;]*m", '') -split "`r?`n" | Where-Object { $_ -match 'failed' } | Select-Object -Last 1)
        Stop-Step "Synaps didn't take the $($b.Name) key: $(("$why".Trim() -replace '^[^\w]*Login failed:\s*', ''))"
    }
    Good "your $($b.Name) key is in Synaps (its credential store)"
    Result 'brain: yes'
}

# Synaps' config value for Key: its last `key = value` line, as Synaps reads it (# lines skipped);
# '' when there's none.
function Get-ConfigValue([string]$Text, [string]$Key) {
    $v = ''
    foreach ($l in ($Text -split "`r?`n")) {
        $t = $l.Trim()
        if ($t.StartsWith('#') -or -not $t.Contains('=')) { continue }
        $k, $rest = $t -split '=', 2
        if ($k.Trim() -eq $Key) { $v = $rest.Trim().Trim('"') }
    }
    return $v
}

# A model on this PC: -Url (Ollama's when none) as Synaps' provider.local.url when it uses another
# now (its config's line, else Ollama's), written as Synaps' own writer leaves it (`key = value`,
# the line replaced, else added); its daemon reloaded when it runs (it reads its config as it
# starts). Then her model, local/-Model.
function Set-Local {
    $m = "$Model".Trim()
    if ($m.StartsWith('local/')) { $m = $m.Substring(6) }
    if (-not $m) { Stop-Step 'which model? (-Model)' }
    if ($m -cnotmatch $script:ModelId) { Stop-Step "-Model isn't a model's name (letters, digits and ._:/@+- only)" }
    $want = if ($Url) { $Url.Trim().TrimEnd('/') } else { $script:Servers[0].Url }
    if ($want -notmatch '^https?://[^\s"''#\\]+$') { Stop-Step "-Url isn't a server's address (http://host:port/v1)" }
    $cfg = Join-Path (Get-SynapsHome) 'config'
    $text = if (Test-Path -LiteralPath $cfg) { [IO.File]::ReadAllText($cfg) } else { '' }
    $now = Get-ConfigValue $text 'provider.local.url'
    $have = if ($now) { $now.TrimEnd('/') } else { $script:Servers[0].Url }
    if ($have -ne $want) {
        if ($DryRun) { Say "`$ provider.local.url = $want ($cfg)" }
        else {
            $line = "provider.local.url = $want"
            $found = $false
            $lines = New-Object Collections.Generic.List[string]
            foreach ($l in $(if ($text) { @($text -split "`r?`n") } else { @() })) {
                $t = $l.Trim()
                if (-not $t.StartsWith('#') -and $t.Contains('=') -and ($t -split '=', 2)[0].Trim() -eq 'provider.local.url') { $found = $true; $lines.Add($line) }
                else { $lines.Add($l) }
            }
            if ($lines.Count -and $lines[$lines.Count - 1] -eq '') { $lines.RemoveAt($lines.Count - 1) }
            if (-not $found) { $lines.Add($line) }
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $cfg) | Out-Null
            [IO.File]::WriteAllText("$cfg.tmp", (($lines -join "`n") + "`n"), (New-Object Text.UTF8Encoding $false))
            Move-Item -Force -LiteralPath "$cfg.tmp" -Destination $cfg
            Good "Synaps' local server: $want (provider.local.url in $cfg)"
            $s = Find-Synaps
            if ($s.State -ne 'missing' -and (Get-DaemonStatus $s.Path).Alive) {
                $ErrorActionPreference = 'Continue'
                $o = (& $s.Path daemon reload 2>&1 | Out-String)
                if ($LASTEXITCODE) { Warn "its daemon runs with the old one: reload it (synaps daemon reload): $("$o".Trim() -split "`r?`n" | Select-Object -Last 1)" }
                else { Good 'its daemon reloaded it' }
                $ErrorActionPreference = 'Stop'
            }
        }
    }
    $script:Model = "local/$m"
    Set-HerModel
}

# Her exe (-Exe) with these arguments, no window: its exit code, stdout lines and stderr.
function Invoke-Her([string[]]$ArgList) {
    $pi = New-Object Diagnostics.ProcessStartInfo
    $pi.FileName = $Exe
    $pi.Arguments = ($ArgList | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }) -join ' '
    $pi.UseShellExecute = $false
    $pi.CreateNoWindow = $true
    $pi.RedirectStandardOutput = $true
    $pi.RedirectStandardError = $true
    $p = [Diagnostics.Process]::Start($pi)
    $err = $p.StandardError.ReadToEndAsync()
    $out = $p.StandardOutput.ReadToEnd()
    $p.WaitForExit()
    return @{ Code = $p.ExitCode; Out = @($out -split "`r?`n" | Where-Object { $_ }); Err = "$($err.Result)" }
}

# Why synaps-avatar --set saved nothing: its own words (Said: its last `synaps-avatar: ` line), else
# its exit code's (2 a bad --set, 3 settings.toml refused: a typo in it, or unreadable, never written
# over; 4 not saved).
function Get-SetWhy([int]$Code, [string]$Said) {
    $Said = (($Said -split "`r?`n" | Where-Object { $_ -match 'synaps-avatar: ' } | Select-Object -Last 1) -replace '^.*synaps-avatar: ', '').Trim()
    if ($Said -match 'unknown argument `--set`') { return "this build of her can't save settings from her installer yet (synaps-avatar --set)" }
    $settings = Join-Path $env:APPDATA 'synaps-avatar\settings.toml'
    if (-not $Said) {
        return $(switch ($Code) {
                3 { "your settings file ($settings) has a typo, and she won't write over it: fix it, then run this again" }
                4 { "she couldn't write $settings" }
                default { "synaps-avatar --set exited $Code" }
            })
    }
    if ($Code -eq 3) { return "$Said Fix it ($settings), then run this again." }
    return $Said
}

# Her model: -Model, else -Provider's default from her own table (synaps-avatar --providers: never a
# copy here), by her settings writer.
function Set-HerModel {
    Resolve-Her
    if ($DryRun -and -not $Exe) {
        Say "`$ synaps-avatar --set session.model=$(if ($Model) { $Model } else { "($Provider's default)" })"
        Result 'model: dry-run'
        return
    }
    if (-not $Exe) { Stop-Step "there's no synaps-avatar.exe here to set her model with" }
    $m = $Model
    if (-not $m) {
        $b = Find-Brain $Provider
        $m = $b.Model
        if (-not $m) { Stop-Step "this build of her has no default model for $($b.Name) (synaps-avatar --providers)" }
    }
    if ($DryRun) { Say "`$ synaps-avatar --set session.model=$m"; Result "model: $m"; return }
    $r = Invoke-Her @('--set', "session.model=$m")
    if ($r.Code -ne 0) { Stop-Step "her model wasn't saved: $(Get-SetWhy $r.Code $r.Err)" }
    Good "her model: $m"
    Result "model: $m"
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
    $parts = @('tools\make-pack', 'tools\desktop-art.json', 'tools\capricore-outfit.json', 'packs\capri\moods.json', 'packs\capri\persona.md') | ForEach-Object { Join-Path $script:Root $_ }
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
    Resolve-Her
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
        if ($l -match 'cannot download|no checksum-matching pose-sleep|CapriCore v1.2.2 file') { Stop-Step "couldn't download lachy's desktop art or CapriCore's newer frames from GitHub. Is this PC offline? ($l)" }
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
        Stop-Step "her voice is installed, but your choices weren't saved: $(Get-SetWhy $p.ExitCode $said)"
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
