<#
.SYNOPSIS
  Her shots on a Windows runner (.github/workflows/shots.yml). Per take: a fake Synaps daemon
  (tools/fakedaemon.py, so she isn't OFFLINE; FAKE: no model), the fake voice sidecar where a take
  wants voice (tools/fakevoice.py), her own settings.toml and folders; she is driven through her
  control socket, and keys or clicks (tools/shoot-win.ps1) only where it can't. Each step is shot
  cropped to her windows (her, the Line, the bubble, the menu), its `state` beside it.

  pwsh tools/shots/scenes.ps1 -Exe her.exe -Pack packs\capri -Out shots\100 [-Sets settings,header,brain] [-Only settings/on,header/hf-470]

  Sets:
    settings  her Settings page (#77): chat, ctrl+, then down (past BRAIN), right, down, esc; her menu;
              voice off; brain off; [brain] enabled = false by a reload; CAN'T SAVE on a read-only
              settings.toml; the cog and `CHAT` (their rects, from `state`) with an empty chat and a full one.
    header    the bubble's header (#78) at 470 px and at 340 px: hands-free and a reply, a prompt
              with 3 waiting, his long words (YOU) while the turn runs.
    brain     her brain (Settings PR 3), the daemon found as on his machine (its daemon.json in a
              Synaps folder of the take's own, no --socket): no login (her first words, SET ME UP,
              the Brain screen it opens, OTHER, esc, her menu); Claude with no login after his first
              message (the tag, a click on her, the list's BRAIN row); signed in (the list, the Brain
              screen, MODEL changed and written); his TUI's /model followed, a ChatGPT sign-in made in
              a terminal seen by the open screen, MODEL sent back (PR 4); a Groq key on file, API
              KEY's enter switching her to it (PR 5); and a daemon through --socket (another
              machine's).
  Out: <Out>\<set>\<take>\<step>.png + .json (her `state`), <Out>\index.json, <Out>\logs\.
#>
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$Pack,
    [Parameter(Mandatory)][string]$Out,
    [string[]]$Sets = @('settings', 'header', 'brain'),
    [string[]]$Only = @(),
    [string]$Python = 'python',
    [int]$FakePort = 47811
)
$ErrorActionPreference = 'Stop'
$Tools = Split-Path -Parent $PSScriptRoot
. (Join-Path $Tools 'shoot-win.ps1')
$Fixtures = Join-Path (Split-Path -Parent $Tools) 'tests\fixtures'
$Root = Join-Path ($env:RUNNER_TEMP ?? $env:TEMP) "sa-shots-$PID"
$Out = [IO.Path]::GetFullPath($Out)
$Logs = Join-Path $Out 'logs'
New-Item -ItemType Directory -Force -Path $Logs | Out-Null
$script:Index = [System.Collections.Generic.List[object]]::new()
$script:Fails = [System.Collections.Generic.List[string]]::new()

function Q([string]$s) { '"' + $s + '"' }

# ---- her control socket and the fake daemon's ----------------------------------------------------

<# One command to her control socket (AF_UNIX, as capri-ctl): her reply. #>
function Ctl([string]$Line, [int]$TimeoutMs = 5000) {
    $s = [System.Net.Sockets.Socket]::new([System.Net.Sockets.AddressFamily]::Unix, [System.Net.Sockets.SocketType]::Stream, [System.Net.Sockets.ProtocolType]::Unspecified)
    try {
        $s.ReceiveTimeout = $TimeoutMs
        $s.Connect([System.Net.Sockets.UnixDomainSocketEndPoint]::new($script:CtlSock))
        $null = $s.Send([Text.Encoding]::UTF8.GetBytes($Line.Replace("`n", '\n') + "`n"))
        $buf = [byte[]]::new(1 -shl 16)
        $ms = [IO.MemoryStream]::new()
        do {
            $n = $s.Receive($buf)
            if ($n -gt 0) { $ms.Write($buf, 0, $n) }
        } while ($n -gt 0 -and $buf[$n - 1] -ne 10)
        [Text.Encoding]::UTF8.GetString($ms.ToArray()).Trim()
    } finally {
        $s.Dispose()
    }
}

function State { Ctl 'state' | ConvertFrom-Json }

<# One line to the fake daemon's control (FAKE: say TEXT, confirm, confirm2, benign, ...). #>
function Fake([string]$Line) {
    $c = [System.Net.Sockets.TcpClient]::new('127.0.0.1', $FakePort)
    try {
        $st = $c.GetStream()
        $b = [Text.Encoding]::UTF8.GetBytes($Line + "`n")
        $st.Write($b, 0, $b.Length)
        [IO.StreamReader]::new($st).ReadLine()
    } finally {
        $c.Dispose()
    }
}

function Wait-Until([scriptblock]$Ok, [int]$Seconds, [string]$What) {
    $end = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $end) {
        try { if (& $Ok) { return $true } } catch { }
        Start-Sleep -Milliseconds 250
    }
    Write-Warning "timed out ($Seconds s): $What"
    return $false
}

# ---- a take: her rig up, steps, down ---------------------------------------------------------

$script:EnvSet = @()
function Set-TakeEnv([hashtable]$Vars) {
    foreach ($k in $Vars.Keys) {
        Set-Item -Path "env:$k" -Value ([string]$Vars[$k])
        $script:EnvSet += $k
    }
}

function Start-Take {
    param(
        [Parameter(Mandatory)][string]$Set, [Parameter(Mandatory)][string]$Name,
        [string]$Settings = '', [hashtable]$Env = @{}, [object[]]$History, [switch]$Voice, [int]$DeltaMs = 30, [string[]]$HerArgs = @(),
        # The daemon found as on his machine: a Synaps folder of the take's own (SYNAPS_BASE_DIR) with
        # its daemon.json, these files in it (auth.json: a login), and no --socket.
        [switch]$Local, [hashtable]$Synaps = @{}, [string]$Model = ''
    )
    $script:Take = "$Set/$Name"
    $t = Join-Path $Root "$Set-$Name"
    Remove-Item -Recurse -Force $t -ErrorAction SilentlyContinue
    foreach ($d in 'cfg\synaps-avatar', 'data', 'state', 'run', 'fake', 'voice') { New-Item -ItemType Directory -Force -Path (Join-Path $t $d) | Out-Null }
    $script:SettingsFile = Join-Path $t 'cfg\synaps-avatar\settings.toml'
    Set-Content -Path $script:SettingsFile -Value ("[intro]`ncinematic = false`n" + $Settings) -Encoding utf8NoBOM
    $script:CtlSock = Join-Path $t 'run\synaps-avatar.sock'
    $script:TakeOut = Join-Path $Out "$Set\$Name"
    New-Item -ItemType Directory -Force -Path $script:TakeOut | Out-Null
    $log = Join-Path $Logs "$Set-$Name"

    # The fake daemon (FAKE: canned replies, scripted prompts).
    $fakeEnv = @{ FAKE_DIR = (Join-Path $t 'fake'); FAKE_CTL_PORT = $FakePort; FAKE_DELTA_MS = $DeltaMs; PYTHONUTF8 = '1' }
    if ($Model) { $fakeEnv.FAKE_MODEL = $Model }
    if ($null -ne $History) {
        $h = Join-Path $t 'history.json'
        Set-Content -Path $h -Value (ConvertTo-Json -InputObject @($History) -Depth 10) -Encoding utf8NoBOM
        $fakeEnv.FAKE_HISTORY = $h
    }
    Set-TakeEnv $fakeEnv
    $script:Daemon = Start-Process $Python -ArgumentList (Q (Join-Path $Tools 'fakedaemon.py')), (Q $Fixtures) -NoNewWindow -PassThru `
        -RedirectStandardOutput "$log-daemon.log" -RedirectStandardError "$log-daemon.err"
    $null = Wait-Until { (Get-Content -Raw "$log-daemon.log" -ErrorAction SilentlyContinue) -match 'fake daemon on' } 20 'the fake daemon'
    Remove-Item env:FAKE_HISTORY -ErrorAction SilentlyContinue
    Remove-Item env:FAKE_MODEL -ErrorAction SilentlyContinue

    # The fake sidecar: her voice link (loopback TCP, endpoint.json), no audio.
    $script:VoiceProc = $null
    $herEnv = @{
        XDG_CONFIG_HOME = (Join-Path $t 'cfg'); XDG_DATA_HOME = (Join-Path $t 'data'); XDG_STATE_HOME = (Join-Path $t 'state')
        XDG_RUNTIME_DIR = (Join-Path $t 'run'); SA_LAYOUT_PROBE = '1'; RUST_BACKTRACE = '1'
    }
    if ($Voice) {
        $ep = Join-Path $t 'voice\endpoint.json'
        $script:VoiceProc = Start-Process $Python -ArgumentList (Q (Join-Path $Tools 'fakevoice.py')), (Q $ep) -NoNewWindow -PassThru `
            -RedirectStandardOutput "$log-voice.log" -RedirectStandardError "$log-voice.err"
        $null = Wait-Until { Test-Path $ep } 20 'the fake sidecar'
        $herEnv.CAPRI_VOICE_ENDPOINT = $ep
        $herEnv.CAPRI_VOICE_SPAWN = '0'
    }
    $sock = Join-Path $t 'fake\d.sock'
    $where = @('--socket', (Q $sock))
    if ($Local) {
        $base = Join-Path $t 'synaps'
        New-Item -ItemType Directory -Force -Path (Join-Path $base 'run') | Out-Null
        foreach ($f in $Synaps.Keys) { Set-Content -Path (Join-Path $base $f) -Value $Synaps[$f] -Encoding utf8NoBOM }
        $info = [ordered]@{ pid = $script:Daemon.Id; protocol_version = 3; daemon_version = '0.0.0-fake'; socket = $sock; exe = 'C:\fake\synaps.exe' }
        Set-Content -Path (Join-Path $base 'run\daemon.json') -Value (ConvertTo-Json $info) -Encoding utf8NoBOM
        $herEnv.SYNAPS_BASE_DIR = $base
        $where = @()
    }
    foreach ($k in $Env.Keys) { $herEnv[$k] = $Env[$k] }
    Set-TakeEnv $herEnv

    $argv = @('--pack', (Q $Pack)) + $where + @($HerArgs | Where-Object { $_ })
    $script:Her = Start-Process $Exe -ArgumentList $argv -NoNewWindow -PassThru `
        -RedirectStandardOutput "$log-her.log" -RedirectStandardError "$log-her.err"
    $null = $script:Her.Handle  # held, so her exit code is still there after she's gone
    # Her frames in, or her gone (a crash at start: said now, not after the 120 s).
    $up = Wait-Until { $script:Her.HasExited -or (State).loaded -eq $true } 120 "her frames ($script:Take)"
    if ($script:Her.HasExited) { throw "$script:Take: she exited at start (code $($script:Her.ExitCode); see $log-her.err)" }
    if (-not $up) { throw "$script:Take: she never loaded (see $log-her.err)" }
    $live = Wait-Until { (State).session.live -eq $true } 30 "her session ($script:Take)"
    if (-not $live) { $script:Fails.Add("$script:Take: OFFLINE (the fake daemon never answered)") }
    # Her window whole on the screen (her bubble keeps to the part of it there), off the desktop's
    # corner watermark where the Line still fits left of her.
    $st = State
    $sf = [double]$st.scale_factor
    $x = [int][Math]::Min(1390 * $sf, [ShootWin.Native]::Screen()[2] - 24 - $st.window.w / 2)
    $null = Ctl "pos $x 100000 now"
    Start-Sleep -Seconds 1
}

function Stop-Take {
    try { $null = Ctl 'quit' 2000 } catch { }
    if ($script:Her -and -not $script:Her.WaitForExit(5000)) { Stop-Process -Id $script:Her.Id -Force -ErrorAction SilentlyContinue }
    try { $null = Fake 'quit' } catch { }
    foreach ($p in $script:Daemon, $script:VoiceProc) {
        if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    }
    foreach ($k in $script:EnvSet) { Remove-Item "env:$k" -ErrorAction SilentlyContinue }
    $script:EnvSet = @()
    Start-Sleep -Milliseconds 500
}

<# Dialogs closed (the runner's, and hers: a failure, said in `fails`). #>
function Close-Dialogs([string]$Where) {
    foreach ($d in [ShootWin.Native]::CloseDialogs()) {
        Write-Host "desktop: closed a dialog: '$($d.Title)' (pid $($d.Pid))"
        if ($script:Her -and $d.Pid -eq $script:Her.Id) { $script:Fails.Add("${Where}: her dialog '$($d.Title)' (closed)") }
    }
}

<# A step's shot: her windows, cropped; her `state` beside it. #>
function Shot([string]$Step, [string]$Note = '') {
    # A notice the runner popped up meanwhile (the scaling change's) goes before it is in one. One
    # of hers (an error box) is a failure, closed or not.
    Close-Dialogs "$script:Take/$Step"
    Start-Sleep -Milliseconds 400
    $png = Join-Path $script:TakeOut "$Step.png"
    $rect = $null
    try {
        $rect = Save-WindowShot -Path $png -ProcessId $script:Her.Id -Pad 16
    } catch {
        $script:Fails.Add("$script:Take/${Step}: $_")
    }
    $raw = Ctl 'state'
    Set-Content -Path (Join-Path $script:TakeOut "$Step.json") -Value $raw -Encoding utf8NoBOM
    $s = $raw | ConvertFrom-Json
    $panel = $s.panel_layout.panel
    $script:Index.Add([pscustomobject]@{
            take = $script:Take; step = $Step; note = $Note; png = "$($script:Take)/$Step.png"; rect = $rect
            live = $s.session.live; sf = $s.scale_factor
            line = if ($s.line) { [pscustomobject]@{ page = $s.line.page; height = $s.line.height; spot = $s.line.spot; at = $s.line.at; focused = $s.line.focused } } else { $null }
            bubble_w = if ($panel) { $panel[2] } else { $null }
            header = $s.panel_layout.header_px
        })
    Write-Host "  $script:Take/$Step $(if ($rect) { $rect -join ',' } else { '(no shot)' })"
}

<# The Line takes the keyboard: a click on its field (it opens parked from her socket). #>
function Focus-Line {
    for ($i = 0; $i -lt 3; $i++) {
        $s = State
        $l = $s.line
        if (-not $l) { throw "$script:Take: no Line to focus" }
        if ($l.focused -and -not $l.parked) { return }
        $sf = [double]$s.scale_factor
        Send-ShotClick -X ([int]($l.at[0] + $l.at[2] * 0.6)) -Y ([int]($l.at[1] + $l.at[3] - 64 * $sf))
        Start-Sleep -Milliseconds 300
        Move-ShotPointer 0 0
        Start-Sleep -Milliseconds 500
    }
    $script:Fails.Add("$script:Take: the Line never took the keyboard")
}

<# The same by a click on the Line's header (nothing there acts): on Settings > Brain a click near
   its bottom could take a way in. #>
function Focus-Line-Top {
    for ($i = 0; $i -lt 3; $i++) {
        $s = State
        $l = $s.line
        if (-not $l) { throw "$script:Take: no Line to focus" }
        if ($l.focused -and -not $l.parked) { return }
        $sf = [double]$s.scale_factor
        Send-ShotClick -X ([int]($l.at[0] + $l.at[2] * 0.6)) -Y ([int]($l.at[1] + 16 * $sf))
        Start-Sleep -Milliseconds 300
        Move-ShotPointer 0 0
        Start-Sleep -Milliseconds 500
    }
    $script:Fails.Add("$script:Take: the Line never took the keyboard")
}

function Key([string]$Chord, [int]$AfterMs = 900) {
    Send-ShotKeys $Chord
    Start-Sleep -Milliseconds $AfterMs
}

<# Click one of her `state` buttons (root px), e.g. menu-attention. #>
function Click-Button([string]$Id) {
    $b = (State).buttons.$Id
    if (-not $b) { throw "$script:Take: no button $Id in state" }
    Send-ShotClick -X ([int]($b[0] + $b[2] / 2)) -Y ([int]($b[1] + $b[3] / 2))
}

# ---- the scenes ----------------------------------------------------------------------------------

$Reply = 'Morning. CI is green on main and the bubble fix is in. Want the summary, or the diff?'
$Short = @(
    @{ role = 'user'; content = 'is the build green yet?' },
    @{ role = 'assistant'; content = @(@{ type = 'text'; text = 'Green as of two minutes ago. Tests passed, nobody cried.' }) }
)
$Full = $Short + @(
    @{ role = 'user'; content = 'what did spike push last night?' },
    @{ role = 'assistant'; content = @(@{ type = 'text'; text = 'The Settings page: a cog left of the field, attention and the wake word, and a fix so her menu never lands on her face.' }) },
    @{ role = 'user'; content = 'and the windows runner?' },
    @{ role = 'assistant'; content = @(@{ type = 'text'; text = 'Green. Build, tests and clippy, plus the installer round trip.' }) },
    @{ role = 'user'; content = 'ok, ping me when shady has looked' },
    @{ role = 'assistant'; content = @(@{ type = 'text'; text = 'Will do.' }) }
)
$VoiceOn = "[voice]`nenabled = true`n"

# Each take: its name and a scriptblock (Start-Take ... steps ... ), run in order.
$Scenes = [ordered]@{
    settings = [ordered]@{
        on       = {
            Start-Take settings on -Settings $VoiceOn -History $Short -Voice
            $null = Ctl 'brain off'
            $null = Fake "say $Reply"
            Start-Sleep 4
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line
            Shot '1-chat' 'her Line, a reply in it'
            Key 'ctrl+comma' 1400; Shot '2-settings' 'ctrl+,'
            Key 'down'; Key 'right' 1400; Shot '3-change' 'then down (past BRAIN), right'
            Key 'down'; Shot '4-down' 'then down'
            Key 'escape' 1400; Shot '5-esc' 'then esc'
            Key 'escape' 1400
            $null = Ctl 'menu'
            Start-Sleep 1
            Shot '6-menu' 'line closed, her menu'
        }
        off      = {
            Start-Take settings off -Settings "[voice]`nenabled = false`n" -History $Short
            $null = Ctl 'brain off'
            $null = Fake "say $Reply"
            Start-Sleep 4
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line
            Key 'ctrl+comma' 1400; Key 'down'; Key 'down'; Key 'right' 1400
            Shot '7-locked' 'voice off in settings.toml: ctrl+, down down right'
        }
        nobrain  = {
            Start-Take settings nobrain -Env @{ SA_NO_BRAIN = '1' } -History $Short
            $null = Fake "say $Reply"
            Start-Sleep 4
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line
            Key 'ctrl+comma' 1400; Key 'down'; Key 'right' 1400
            Shot '8-nobrain' 'brain off: ctrl+, down right'
        }
        reload   = {
            Start-Take settings reload -History $Short
            $null = Ctl 'brain off'
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line
            Key 'ctrl+comma' 1400
            Shot '9-before' 'Settings, her brain on'
            Add-Content -Path $script:SettingsFile -Value "[brain]`nenabled = false`n" -Encoding utf8NoBOM
            $r = Ctl 'settings reload'
            Write-Host "  settings reload: $r"
            Start-Sleep 1.5
            Shot '10-reloaded' '[brain] enabled = false, settings reload'
            Key 'escape' 1000; Key 'escape' 1000
            $null = Ctl 'menu'
            Start-Sleep 1
            Shot '11-menu' 'her menu after it: no ATTENTION row'
        }
        cantsave = {
            Start-Take settings cantsave -History $Short
            $null = Ctl 'brain off'
            Set-ItemProperty -Path $script:SettingsFile -Name IsReadOnly -Value $true
            $null = Ctl 'menu'
            Start-Sleep 1
            Shot '12-menu' 'her menu, settings.toml read-only'
            Click-Button 'menu-attention'
            Start-Sleep 1.2
            Shot '13-cantsave' 'clicked ATTENTION: CANT SAVE'
            Set-ItemProperty -Path $script:SettingsFile -Name IsReadOnly -Value $false
        }
        empty    = {
            Start-Take settings empty -History @()
            $null = Ctl 'brain off'
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line
            Shot '14-empty-chat' 'an empty chat: the cog'
            Key 'ctrl+comma' 1400; Shot '15-empty-settings' 'ctrl+,: CHAT where the cog was'
            Key 'escape' 1400; Shot '16-empty-back' 'esc: back on chat'
        }
        full     = {
            Start-Take settings full -History $Full
            $null = Ctl 'brain off'
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line
            Shot '17-full-chat' 'a full chat: the cog'
            Key 'ctrl+comma' 1400; Shot '18-full-settings' 'ctrl+,: CHAT where the cog was'
        }
    }
    header   = [ordered]@{}
    brain    = [ordered]@{
        # No login for her model (ChatGPT's: a file only, so a read tells even on Windows, where the
        # daemon's environment can't be read): her first words say so, with SET ME UP.
        first  = {
            Start-Take brain first -Local -Model 'openai-codex/gpt-5.5' -History @()
            $null = Ctl 'brain off'
            $null = Wait-Until { (State).buttons.setup } 30 'her first words with SET ME UP'
            Start-Sleep 2.5
            Shot '01-first-words' 'no login: her first words, SET ME UP'
            Click-Button 'setup'
            Start-Sleep 2
            Focus-Line-Top
            Shot '02-set-me-up' 'SET ME UP: the Line on Settings > Brain, Claude picked'
            Key 'down'; Key 'down'; Key 'enter' 1400
            Shot '03-other' 'down down enter: OTHER open in place'
            Key 'escape' 1400
            Shot '04-esc' 'esc: chat (it opened on Brain)'
            Key 'escape' 1400
            $null = Ctl 'menu'
            Start-Sleep 1
            Shot '05-menu' 'her menu: SET UP MY BRAIN'
        }
        # Claude, no login: the first message fails as Synaps' does (FAKE: nobrain).
        none   = {
            Start-Take brain none -Local -Model 'claude-sonnet-4-6' -History $Short
            $null = Ctl 'brain off'
            $null = Fake 'nobrain'
            Start-Sleep 3
            $null = Ctl 'submit hi'
            $null = Wait-Until { (State).session.label -eq 'NO BRAIN YET' } 20 'NO BRAIN YET'
            Start-Sleep 3
            Shot '06-tag' 'his first message: NO BRAIN YET'
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line-Top
            Shot '07-summon' 'the summon: Settings > Brain'
            Key 'escape' 1400
            Key 'ctrl+comma' 1400
            Shot '08-list' 'ctrl+,: the list, BRAIN first: NO BRAIN YET, SET UP'
        }
        # Signed in to Claude (a login on file): the list, the Brain screen, MODEL changed.
        signed = {
            Start-Take brain signed -Local -Model 'claude-sonnet-4-6' -History $Short -Synaps @{ 'auth.json' = '{"anthropic": {"type": "oauth"}}' }
            $null = Ctl 'brain off'
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line-Top
            Key 'ctrl+comma' 1400
            Shot '09-list' 'signed in: the list, BRAIN first'
            Key 'enter' 1400
            Shot '10-brain' 'enter: the Brain screen, Claude in use, MODEL'
            Key 'down'; Key 'down'; Key 'down'; Key 'right' 1800
            Shot '11-model' 'down to MODEL, right: OPUS 5.5, written'
            $f = Get-Content -Raw $script:SettingsFile
            Write-Host "  settings.toml: $($f -replace "`n", ' | ')"
            if ($f -notmatch '(?m)^model = "claude-opus-5-5"') { $script:Fails.Add("${script:Take}: [session] model not written: $f") }
            Key 'escape' 1400
            Shot '12-back' 'esc: back on the list'
        }
        # Her session moved by another client (his TUI's /model: the daemon's broadcast), then a sign-in
        # to ChatGPT in his terminal while the Brain screen is open, then MODEL back to Sonnet.
        drift  = {
            Start-Take brain drift -Local -Model 'claude-sonnet-4-6' -History $Short -Synaps @{ 'auth.json' = '{"anthropic": {"type": "oauth"}}' }
            $null = Ctl 'brain off'
            $null = Ctl 'settings open brain'
            Start-Sleep 2
            Focus-Line-Top
            $null = Fake 'model claude-haiku-4-5-20251001'
            if (-not (Wait-Until { (State).line.mind.model -eq 'claude-haiku-4-5-20251001' } 10 'MODEL following his TUI')) {
                $script:Fails.Add("${script:Take}: MODEL never followed his TUI")
            }
            Start-Sleep 1
            Shot '14-tui-model' 'his TUI''s /model haiku: MODEL says HAIKU 4.5'
            Key 'down'; Key 'down'; Key 'enter' 1400
            Set-Content -Path (Join-Path $Root 'brain-drift\synaps\auth.json') -Value '{"anthropic": {"type": "oauth"}, "openai-codex": {"type": "oauth"}}' -Encoding utf8NoBOM
            $signed = Wait-Until { @((State).line.mind.signed) -contains 'openai-codex' } 12 'a ChatGPT sign-in made in his terminal'
            if (-not $signed) { $script:Fails.Add("${script:Take}: the open Brain screen never saw ChatGPT's login") }
            Key 'down' 1000
            Shot '15-terminal-signin' 'ChatGPT signed in from a terminal: its check without reopening; the keys say what enter does'
            Key 'left'; Key 'down'; Key 'right' 1800
            $dl = Join-Path $Logs 'brain-drift-daemon.log'
            $sent = Wait-Until { (Get-Content -Raw $dl -ErrorAction SilentlyContinue) -match 'cmd set model claude-sonnet-4-6' } 10 'MODEL sent back to her session'
            if (-not $sent) { $script:Fails.Add("${script:Take}: MODEL back to Sonnet never reached her session (stale view)") }
            Start-Sleep 1
            Shot '16-model-back' 'left, down to MODEL, right: SONNET 4.6, set on her session again'
        }
        # Claude signed in, a Groq key on file: API KEY says KEY SET and enter switches her to it.
        keys   = {
            Start-Take brain keys -Local -Model 'claude-sonnet-4-6' -History $Short -Synaps @{ 'auth.json' = '{"anthropic": {"type": "oauth"}, "groq": {"type": "api_key", "key": "gsk-fake"}}' }
            $null = Ctl 'brain off'
            $null = Ctl 'settings open brain'
            Start-Sleep 2
            Focus-Line-Top
            Key 'down'; Key 'down'; Key 'enter' 1000
            # API KEY is the row above MODEL, OTHER open: to the bottom, one up.
            1..12 | ForEach-Object { Key 'down' 150 }
            Key 'up' 1200
            if (-not (Wait-Until { @((State).line.mind.signed) -contains 'groq' } 10 'the Groq key read')) {
                $script:Fails.Add("${script:Take}: the Brain screen never saw the Groq key")
            }
            Start-Sleep 1
            Shot '17-key-set' 'a Groq key on file: API KEY says KEY SET; the keys say enter uses it'
            Key 'enter' 1800
            $dl = Join-Path $Logs 'brain-keys-daemon.log'
            if (-not (Wait-Until { (Get-Content -Raw $dl -ErrorAction SilentlyContinue) -match 'cmd set model groq/llama-3.3-70b-versatile' } 10 'her session on Groq')) {
                $script:Fails.Add("${script:Take}: enter on API KEY never switched her to Groq")
            }
            Start-Sleep 1
            Shot '18-key-in-use' 'enter: her brain is Groq, API KEY says IN USE'
        }
        # A daemon through --socket (maybe another machine's): its logins aren't hers to read or make.
        remote = {
            Start-Take brain remote -Model 'claude-sonnet-4-6' -History $Short
            $null = Ctl 'brain off'
            $null = Ctl 'settings open brain'
            Start-Sleep 2
            Focus-Line-Top
            Shot '13-remote' 'a daemon through --socket: settings open brain'
        }
    }
}

# The header at two widths: 470 where she stands (a reply long enough to fill it), 340 against the
# screen's left edge: her bubble keeps to the part of her window on the screen, and her window is
# 500 px with her in its middle, so she is drawn 190 px tall there (--height) to leave it 352 px
# (the bubble's least, 340, then).
$HfReply = 'Spike pushed the reader fix eleven minutes ago, and CI went green right after. The Windows installer job took nineteen minutes and the voice bundle eleven. Want me to nudge lynx about the rebase, or leave it for the morning?'
$YouSay = 'can you check whether the windows runner still builds the release binary after the vendored gpui patch from last night'
foreach ($w in 470, 340) {
    $edge = $w -eq 340
    $Scenes.header["hf-$w"] = [scriptblock]::Create(@"
        Start-Take header hf-$w -Settings "[voice]``nenabled = true``nwake_word = false``n" -History `$Short -Voice -HerArgs `$(if (`$$edge) { @('--height', '190') } else { @() })
        `$null = Ctl 'brain off'
        if (`$$edge) { To-Edge }
        `$null = Ctl 'talk handsfree on'
        Start-Sleep 1.5
        `$null = Fake "say `$HfReply"
        Start-Sleep 4
        Shot 'hf' 'hands-free and a reply'
"@)
    $Scenes.header["prompt-$w"] = [scriptblock]::Create(@"
        Start-Take header prompt-$w -Settings "[voice]``nenabled = true``nwake_word = false``n" -History `$Short -Voice -HerArgs `$(if (`$$edge) { @('--height', '190') } else { @() })
        `$null = Ctl 'brain off'
        if (`$$edge) { To-Edge }
        `$null = Ctl 'talk handsfree on'
        Start-Sleep 1.5
        foreach (`$p in 'confirm2', 'confirm', 'benign') { `$null = Fake `$p; Start-Sleep -Milliseconds 300 }
        Start-Sleep 2
        Shot 'prompt' 'a prompt, 3 waiting, hands-free'
"@)
    $Scenes.header["you-$w"] = [scriptblock]::Create(@"
        Start-Take header you-$w -History `$Short -DeltaMs 600 -HerArgs `$(if (`$$edge) { @('--height', '190') } else { @() })
        `$null = Ctl 'brain off'
        if (`$$edge) { To-Edge }
        `$null = Ctl "submit `$YouSay"
        Start-Sleep 2.5
        Shot 'you' 'his long words while the turn runs'
"@)
}

<# Her against the screen's left edge. #>
function To-Edge {
    $null = Ctl 'pos 0 100000 now'
    Start-Sleep 1
    $o = (State).window
    $sc = [ShootWin.Native]::Screen()
    $vis = [Math]::Min($o.x + $o.w, $sc[0] + $sc[2]) - [Math]::Max($o.x, $sc[0])
    Write-Host "  at the edge: her window at $($o.x), $($o.w) wide: $vis px of it on screen"
}

# ---- run ---------------------------------------------------------------------------------------

Clear-ShotDesktop

foreach ($set in $Sets) {
    if (-not $Scenes.Contains($set)) { throw "no scene set '$set' (sets: $($Scenes.Keys -join ', '))" }
    foreach ($name in @($Scenes[$set].Keys)) {
        if ($Only.Count -and "$set/$name" -notin $Only) { continue }
        Write-Host "take $set/$name"
        try {
            & $Scenes[$set][$name]
        } catch {
            $script:Fails.Add("$set/${name}: $_")
            Write-Warning "$set/${name}: $_"
        } finally {
            Stop-Take
        }
    }
}

[pscustomobject]@{ index = $script:Index; fails = $script:Fails; screen = [ShootWin.Native]::Screen(); scales = [ShootWin.Native]::Scales() } |
    ConvertTo-Json -Depth 8 | Set-Content -Path (Join-Path $Out 'index.json') -Encoding utf8NoBOM
Write-Host "$($script:Index.Count) shots, $($script:Fails.Count) problem(s)"
$script:Fails | ForEach-Object { Write-Warning $_ }
# The step fails with any problem (the index and the shots are still kept: the upload runs anyway).
if ($script:Index.Count -eq 0) { throw 'no shots at all' }
if ($script:Fails.Count) { exit 1 }
