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
    tray      her icon in the notification area (#79), promoted out of the overflow, on a dark
              taskbar with its clock off: hovered (its tooltip, capri-syn · ONLINE), right-clicked (her
              menu), cropped to the taskbar's corner and the menu; the menu by his keys (ATTENTION lit,
              its choices open); then QUIT leaves no icon behind.
              Restarts Explorer (a runner only).
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

# The Synaps her fake daemon says it is (her Settings' VERSION row shows it): the one her installer
# pins (installer/manifest's synaps_pin, fakedaemon.py's default too), never a FAKE string on camera.
$DaemonVersion = (Get-Content (Join-Path (Split-Path -Parent $Tools) 'installer\manifest') | Where-Object { $_ -like 'synaps_pin=*' }) -replace '^synaps_pin=', ''

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
    $fakeEnv = @{ FAKE_DIR = (Join-Path $t 'fake'); FAKE_CTL_PORT = $FakePort; FAKE_DELTA_MS = $DeltaMs; FAKE_DAEMON_VERSION = $DaemonVersion; PYTHONUTF8 = '1' }
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
        $info = [ordered]@{ pid = $script:Daemon.Id; protocol_version = 3; daemon_version = $DaemonVersion; socket = $sock; exe = 'C:\fake\synaps.exe' }
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

<# A step's shot: her windows, cropped (or -At: a rect of the screen, x, y, w, h); her `state` beside it. #>
function Shot([string]$Step, [string]$Note = '', [int[]]$At = $null) {
    # A notice the runner popped up meanwhile (the scaling change's) goes before it is in one. One
    # of hers (an error box) is a failure, closed or not.
    Close-Dialogs "$script:Take/$Step"
    Start-Sleep -Milliseconds 400
    $png = Join-Path $script:TakeOut "$Step.png"
    $rect = $null
    try {
        if ($At) {
            Save-RectShot -Path $png -Rect $At
            $rect = $At
        } else {
            $rect = Save-WindowShot -Path $png -ProcessId $script:Her.Id -Pad 16
        }
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
        apikey = {
            Start-Take brain apikey -Local -Model 'claude-sonnet-4-6' -History $Short -Synaps @{ 'auth.json' = '{"anthropic": {"type": "oauth"}, "groq": {"type": "api_key", "key": "gsk-fake"}}' }
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
            $dl = Join-Path $Logs 'brain-apikey-daemon.log'
            if (-not (Wait-Until { (Get-Content -Raw $dl -ErrorAction SilentlyContinue) -match 'cmd set model groq/llama-3.3-70b-versatile' } 10 'her session on Groq')) {
                $script:Fails.Add("${script:Take}: enter on API KEY never switched her to Groq")
            }
            Start-Sleep 1
            Shot '18-key-in-use' 'enter: her brain is Groq, API KEY says IN USE'
        }
        # A local URL set after the daemon started (LM Studio's in its config; the daemon, which reads
        # it at its start, still on Ollama's): its error names the old URL, and the Brain screen says
        # to reload it.
        stale  = {
            Start-Take brain stale -Local -Model 'local/qwen3:8b' -History $Short -Synaps @{ 'config' = "provider.local.url = http://127.0.0.1:9/v1`n" }
            $null = Ctl 'brain off'
            $null = Fake "fail openai request failed: broker transport error: can't reach local endpoint at http://localhost:11434/v1/chat/completions -- is Ollama/LM Studio running?"
            Start-Sleep 3
            $null = Ctl 'submit hi'
            Start-Sleep 3
            $null = Ctl 'settings open brain'
            Start-Sleep 2
            Focus-Line-Top
            if (-not (Wait-Until { (State).line.mind.tried -eq 'http://localhost:11434/v1' -and (State).line.mind.local -like 'http://127.0.0.1:9/v1 *' } 15 'the old URL beside hers')) {
                $script:Fails.Add("${script:Take}: the Brain screen never had the daemon's old URL beside hers")
            }
            Start-Sleep 1
            Shot '19-stale-url' 'the daemon on its old local URL: no check, and synaps daemon reload'
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
    tray     = [ordered]@{
        # Her icon (#79) on a dark taskbar, out of the overflow: its tooltip, then her menu (the
        # one model's rows: voice on for HANDS-FREE TALK, a reply for COPY LAST REPLY). She stands
        # a quarter of the way in, off the corner. QUIT takes the icon out: none left behind.
        menu = {
            Set-ShotTaskbar
            Start-Take tray menu -Settings "[voice]`nenabled = true`nwake_word = false`n" -History $Short -Voice
            $null = Ctl 'brain off'
            $sc = [ShootWin.Native]::Screen()
            $null = Ctl "pos $([int]($sc[0] + $sc[2] / 4)) 100000 now"
            $null = Show-TrayIcon
            $tip = 'capri-syn · ONLINE'
            if (-not (Wait-Until { (Find-TrayButton).Name -ceq $tip } 20 "her tooltip: $tip")) {
                $script:Fails.Add("${script:Take}: her tooltip is '$((Find-TrayButton).Name)', not '$tip'")
            }
            $icon = Find-TrayButton
            $cx = [int]($icon.X + $icon.W / 2); $cy = [int]($icon.Y + $icon.H / 2)
            $sf = [double](State).scale_factor

            # Hovered (real moves): the shell's tooltip, in whatever window it draws it near her icon.
            $before = @([ShotsTray.Native]::Of(0) | ForEach-Object { $_.Handle })
            $null = [ShotsTray.Native]::Glide($cx, $cy)
            Start-Sleep 2.5
            $near = @(New-Windows $before | Where-Object { $_.Y + $_.H -ge $icon.Y - 400 * $sf -and [Math]::Abs($_.X + $_.W / 2 - $cx) -lt 400 * $sf })
            if (-not $near) { $script:Fails.Add("${script:Take}: no tooltip window near her icon") }
            $tipBox = if ($near) { $near } else { @([pscustomobject]@{ X = $cx - [int](180 * $sf); Y = $icon.Y - [int](64 * $sf); W = [int](360 * $sf); H = [int](64 * $sf) }) }
            Shot '01-tip' "hovered: $tip$(if (-not $near) { ' (no tooltip window found: a box above her icon)' })" -At (Get-CornerRect -Icon $icon -Also $tipBox)
            Move-ShotPointer ($sc[0] + 8) ($sc[1] + 8)
            Start-Sleep 1.5

            # Right-clicked (real input): her menu, a #32768 window of hers. If none opens, the shell's
            # own message for it (version 4: WM_CONTEXTMENU at her icon) to her tray's window, said.
            # Clicked in her button's top fifth: the menu ends where it was asked for, over her
            # glyph from its middle.
            $menu = $null
            $ry = [int]($icon.Y + $icon.H / 5)
            for ($i = 1; $i -le 2 -and -not $menu; $i++) {
                $before = @([ShotsTray.Native]::Of(0) | ForEach-Object { $_.Handle })
                $null = [ShotsTray.Native]::Glide($cx, $ry)
                Start-Sleep -Milliseconds 300
                $pressed = [ShotsTray.Native]::Press($true)
                $null = Wait-Until { Get-HerMenu } 5 'her tray menu'
                $menu = Get-HerMenu
                $fg = [ShotsTray.Native]::Front()
                Write-Host "  right click ${i} (SendInput took $pressed of 2): $(if ($menu) { 'her menu' } else { 'no menu' }); in front: $($fg.Class) (pid $($fg.Pid)$(if ($fg.Pid -eq $script:Her.Id) { ', hers' }))"
                if (-not $menu) { $null = New-Windows $before; Move-ShotPointer ($sc[0] + 8) ($sc[1] + 8); Key 'escape' 600 }
            }
            $how = 'a real right click'
            $tw = [ShotsTray.Native]::Find('SynapsAvatarTray', [uint32]$script:Her.Id)
            if (-not $menu) {
                $ok = [ShotsTray.Native]::Post($tw, 0x8001, ($cx -band 0xffff) -bor (($ry -band 0xffff) -shl 16), 0x7B -bor (1 -shl 16))
                $null = Wait-Until { Get-HerMenu } 5 'her tray menu (the shell''s message)'
                $menu = Get-HerMenu
                $how = "the shell's message posted to her tray window ($tw, $ok): a real right click opened none"
                Write-Warning "${script:Take}: $how"
            }
            if (-not $menu) { throw "$script:Take: no menu, by a real right click or the shell's message" }
            # The pointer off it by a jump (a glide across would light a row, or open ATTENTION's).
            Move-ShotPointer ($sc[0] + 8) ($sc[1] + 8)
            Start-Sleep 1
            Write-Host "  her menu at $($menu.X),$($menu.Y) $($menu.W)x$($menu.H), by $how"
            Shot '02-menu' "her menu, by $how" -At (Get-CornerRect -Icon $icon -Also @($menu) -Aspect 1.6 -Above 96)
            # Her rows' names as a screen reader gets them: MSAA's menu proxy (oleacc, on her menu
            # window). UI Automation is said, never failed: on the runner it sees no Win32 menu's
            # rows at all (main's stock menu: the same).
            Show-UiaFocus 'her menu up'
            $read = Get-MenuNames $menu 'her menu'
            foreach ($want in 'RESET POSITION', 'ATTENTION: MIX', 'QUIT') {
                if ($want -cnotin $read.Msaa) { $script:Fails.Add("${script:Take}: no row named '$want' for a screen reader (MSAA reads: $($read.Msaa -join ', '))") }
                if ($want -cnotin $read.Uia3) { Write-Warning "${script:Take}: UI Automation reads no row named '$want' (it reads: $($read.Uia3 -join ', ')); not a failure: it sees no Win32 menu's rows here, main's stock menu's too" }
            }
            # His keys in it (a real right click gave it the keyboard): down x3 lights ATTENTION (a
            # lit row), right opens its choices (the one in force marked, the first lit).
            if ($how -eq 'a real right click') {
                Key 'down' 300
                Show-UiaFocus 'down: the first row lit'
                foreach ($k in 'down', 'down') { Key $k 200 }
                Key 'right' 800
                Show-UiaFocus 'right: into ATTENTION'
                $menus = @([ShotsTray.Native]::Of([uint32]$script:Her.Id) | Where-Object { $_.Class -eq '#32768' })
                if ($menus.Count -ge 2) {
                    Shot '03-keys' 'her menu by his keys: ATTENTION lit, its choices open' -At (Get-CornerRect -Icon $icon -Also $menus -Aspect 1.6 -Above 96)
                    # Its choices by name too (the one in force first: MIX).
                    $sub = $menus | Where-Object { $_.Handle -ne $menu.Handle } | Select-Object -First 1
                    $subRead = Get-MenuNames $sub 'ATTENTION'
                    if ('MIX' -cnotin $subRead.Msaa) { $script:Fails.Add("${script:Take}: no choice named 'MIX' under ATTENTION for a screen reader (MSAA reads: $($subRead.Msaa -join ', '))") }
                }
                else { $script:Fails.Add("${script:Take}: down, down, down, right opened no ATTENTION choices ($($menus.Count) menu)") }
                Key 'escape' 500
            }
            Key 'escape' 800
            if (Get-HerMenu) {
                # Opened by a post, it never had the keyboard (no foreground for her tray's window).
                if ($how -eq 'a real right click') { $script:Fails.Add("${script:Take}: her menu stayed after esc") }
                $null = [ShotsTray.Native]::Post($tw, 0x001F, 0, 0)  # WM_CANCELMODE
                $null = Wait-Until { -not (Get-HerMenu) } 3 'her menu closed'
            }

            # A real left click: the Line (her summon key's tap). Said, never a shot.
            $null = [ShotsTray.Native]::Glide($cx, $cy)
            Start-Sleep -Milliseconds 300
            $null = [ShotsTray.Native]::Press($false)
            Move-ShotPointer ($sc[0] + 8) ($sc[1] + 8)
            if (Wait-Until { (State).line } 5 'the Line, from a left click on her icon') { Write-Host '  left click: the Line opened' }
            else { Write-Warning "${script:Take}: a real left click on her icon opened no Line" }

            # QUIT: the icon goes with her.
            $null = Ctl 'quit' 2000
            if (-not $script:Her.WaitForExit(10000)) { $script:Fails.Add("${script:Take}: she didn't quit") }
            if (Wait-Until { -not (Find-TrayButton) } 10 'her icon gone') { Write-Host '  quit: her icon is gone' }
            else { $script:Fails.Add("${script:Take}: her icon stayed after QUIT") }
            Write-HerTrayLog
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

# ---- the tray (the tray set) -------------------------------------------------------------------

if (-not ('ShotsTray.Native' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace ShotsTray {
public static class Native {
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindow(string cls, string title);
    [DllImport("shell32.dll")] static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER id, out RECT r);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }
    [StructLayout(LayoutKind.Sequential)] struct NOTIFYICONIDENTIFIER { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }

    public class Win { public long Handle; public uint Pid; public string Class; public int X, Y, W, H; }

    static void Aware() { SetThreadDpiAwarenessContext(new IntPtr(-4)); }

    /// Visible top-level windows (of process `pid`; 0: anyone's) with their class, physical px.
    public static List<Win> Of(uint pid) {
        Aware();
        var o = new List<Win>();
        EnumWindows((h, l) => {
            if (!IsWindowVisible(h)) return true;
            uint p; GetWindowThreadProcessId(h, out p);
            if (pid != 0 && p != pid) return true;
            RECT r;
            if (!GetWindowRect(h, out r) || r.R - r.L < 2 || r.B - r.T < 2) return true;
            var c = new StringBuilder(128); GetClassName(h, c, 128);
            o.Add(new Win { Handle = h.ToInt64(), Pid = p, Class = c.ToString(), X = r.L, Y = r.T, W = r.R - r.L, H = r.B - r.T });
            return true;
        }, IntPtr.Zero);
        return o;
    }

    /// A window of `pid`'s of class `cls`, shown or not (her tray's is hidden): 0 if none.
    public static long Find(string cls, uint pid) {
        long found = 0;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != pid) return true;
            var c = new StringBuilder(128); GetClassName(h, c, 128);
            if (c.ToString() != cls) return true;
            found = h.ToInt64();
            return false;
        }, IntPtr.Zero);
        return found;
    }

    /// The window in front: its class and process.
    public static Win Front() {
        Aware();
        var h = GetForegroundWindow();
        uint p; GetWindowThreadProcessId(h, out p);
        var c = new StringBuilder(128); GetClassName(h, c, 128);
        return new Win { Handle = h.ToInt64(), Pid = p, Class = c.ToString() };
    }

    public static bool Post(long h, uint m, long w, long l) { return PostMessage(new IntPtr(h), m, new IntPtr(w), new IntPtr(l)); }

    /// The pointer to x, y (physical px) as a hand moves it: 10 injected absolute moves (the
    /// taskbar's XAML hovers and lights on input, not on SetCursorPos). The moves SendInput took.
    public static uint Glide(int x, int y) {
        Aware();
        POINT p; GetCursorPos(out p);
        int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = GetSystemMetrics(78), vh = GetSystemMetrics(79);
        uint n = 0;
        for (int i = 1; i <= 10; i++) {
            int ix = p.X + (x - p.X) * i / 10, iy = p.Y + (y - p.Y) * i / 10;
            var m = new INPUT { type = 0 };
            m.mi.dx = (int)(((long)(ix - vx) * 65536 + vw - 1) / vw);
            m.mi.dy = (int)(((long)(iy - vy) * 65536 + vh - 1) / vh);
            m.mi.flags = 0x0001 | 0x8000 | 0x4000;  // MOVE | ABSOLUTE | VIRTUALDESK
            n += SendInput(1, new[] { m }, Marshal.SizeOf(typeof(INPUT)));
            System.Threading.Thread.Sleep(15);
        }
        return n;
    }

    /// A button pressed where the pointer is, held ~120 ms (right: the right one). 2: both went in.
    public static uint Press(bool right) {
        var d = new INPUT { type = 0 }; d.mi.flags = right ? 0x0008u : 0x0002u;
        var u = new INPUT { type = 0 }; u.mi.flags = right ? 0x0010u : 0x0004u;
        uint n = SendInput(1, new[] { d }, Marshal.SizeOf(typeof(INPUT)));
        System.Threading.Thread.Sleep(120);
        return n + SendInput(1, new[] { u }, Marshal.SizeOf(typeof(INPUT)));
    }

    /// The taskbar's window (Shell_TrayWnd): 0 while Explorer is down.
    public static long Taskbar() { return FindWindow("Shell_TrayWnd", null).ToInt64(); }

    /// A window's rect: x, y, w, h (physical px); null if it has none.
    public static int[] Rect(long h) {
        Aware();
        RECT r;
        if (h == 0 || !GetWindowRect(new IntPtr(h), out r)) return null;
        return new[] { r.L, r.T, r.R - r.L, r.B - r.T };
    }

    /// An icon's rect by its GUID (the shell's own answer): x, y, w, h, or null (`hr` says why).
    public static int[] IconRect(Guid g, out int hr) {
        Aware();
        var id = new NOTIFYICONIDENTIFIER { guidItem = g };
        id.cbSize = (uint)Marshal.SizeOf(typeof(NOTIFYICONIDENTIFIER));
        RECT r;
        hr = Shell_NotifyIconGetRect(ref id, out r);
        return hr == 0 ? new[] { r.L, r.T, r.R - r.L, r.B - r.T } : null;
    }
}
}
'@
}

<# The taskbar for a still: dark (the shell and apps), opaque, its clock off (the date never on
   camera), its apps to the left (out of a shot of its corner), no toasts; Explorer started again so
   all of it holds. A runner only. #>
function Set-ShotTaskbar {
    Assert-ShotRunner 'Set-ShotTaskbar'
    $cv = 'HKCU:\Software\Microsoft\Windows\CurrentVersion'
    foreach ($v in @(
            @("$cv\Themes\Personalize", 'SystemUsesLightTheme', 0), @("$cv\Themes\Personalize", 'AppsUseLightTheme', 0),
            @("$cv\Themes\Personalize", 'EnableTransparency', 0), @("$cv\Policies\Explorer", 'HideClock', 1),
            @("$cv\Explorer\Advanced", 'TaskbarAl', 0), @("$cv\PushNotifications", 'ToastEnabled', 0), @("$cv\Explorer", 'EnableAutoTray', 0))) {
        if (-not (Test-Path $v[0])) { New-Item -Path $v[0] -Force | Out-Null }
        Set-ItemProperty -Path $v[0] -Name $v[1] -Value $v[2] -Type DWord
    }
    Restart-ShotExplorer -Clear
}

<# Explorer down and up again (winlogon brings the shell back; else it is started): what it reads
   only at its start holds, and every tray icon is asked back (TaskbarCreated). -Clear: the desktop
   quiet again after (Clear-ShotDesktop; never with her up: it minimizes all). #>
function Restart-ShotExplorer {
    param([switch]$Clear)
    Assert-ShotRunner 'Restart-ShotExplorer'
    $old = [ShotsTray.Native]::Taskbar()
    Get-Process explorer -ErrorAction SilentlyContinue | Stop-Process -Force
    $new = { $h = [ShotsTray.Native]::Taskbar(); $h -ne 0 -and $h -ne $old }
    if (-not (Wait-Until $new 15 'the shell back by itself')) {
        Start-Process explorer.exe
        if (-not (Wait-Until $new 30 'the shell back')) { throw 'Explorer never came back' }
    }
    # Its tray up (the TaskbarCreated broadcast).
    Start-Sleep 5
    if ($Clear) { Clear-ShotDesktop }
    Write-Host "  explorer: back; taskbar at $(([ShotsTray.Native]::Rect([ShotsTray.Native]::Taskbar())) -join ',')"
}

<# Everything named on the taskbar (UI Automation, physical px): Name, Class, X, Y, W, H. #>
function Get-TrayButtons {
    if (-not ('System.Windows.Automation.AutomationElement' -as [type])) { Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes }
    [ShootWin.Native]::Aware()
    $h = [ShotsTray.Native]::Taskbar()
    if (-not $h) { return }
    $root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$h)
    foreach ($e in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) {
        $c = $e.Current
        $r = $c.BoundingRectangle
        if (-not $c.Name -or $r.IsEmpty -or $r.Width -lt 1) { continue }
        [pscustomobject]@{ Name = $c.Name; Class = $c.ClassName; X = [int]$r.X; Y = [int]$r.Y; W = [int]$r.Width; H = [int]$r.Height }
    }
}

<# Her icon's button on the taskbar (named by her tooltip; inside the taskbar, not the overflow). #>
function Find-TrayButton {
    $tb = [ShotsTray.Native]::Rect([ShotsTray.Native]::Taskbar())
    if (-not $tb) { return }
    Get-TrayButtons | Where-Object {
        $_.Name -like 'capri-syn*' -and $_.X -ge $tb[0] -and $_.Y -ge $tb[1] - 1 -and $_.X + $_.W -le $tb[0] + $tb[2] -and $_.Y + $_.H -le $tb[1] + $tb[3] + 1
    } | Select-Object -First 1
}

<# Her icon's GUID as she makes it (platform::windows::tray::guid): her exe's path and her control
   socket's (`instance`, as XDG_RUNTIME_DIR is set). #>
function Get-HerTrayGuid {
    $exe = $script:Her.Path.ToLowerInvariant()
    $d = [Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes("capri-syn tray icon`0$exe`0$script:CtlSock"))
    $b = [byte[]]$d[0..15]
    $b[6] = ($b[6] -band 0x0f) -bor 0x50
    $b[8] = ($b[8] -band 0x3f) -bor 0x80
    $x = -join ($b | ForEach-Object { $_.ToString('x2') })
    [Guid]::Parse(('{0}-{1}-{2}-{3}-{4}' -f $x.Substring(0, 8), $x.Substring(8, 4), $x.Substring(12, 4), $x.Substring(16, 4), $x.Substring(20, 12)))
}

<# A NotifyIconSettings key's IconGuid as a [Guid] (text, or 16 bytes); none where it has none. #>
function Get-KeyGuid($Key) {
    $v = $Key.GetValue('IconGuid')
    if ($v -is [byte[]] -and $v.Length -eq 16) { return [Guid]::new([byte[]]$v) }
    $o = [Guid]::Empty
    if ($v -and [Guid]::TryParse([string]$v, [ref]$o)) { return $o }
}

<# Her log's `tray:` lines, said (her stderr's file in the take's logs, and her log file in the take's
   state folder, wherever she wrote them). #>
function Write-HerTrayLog {
    $n = $script:Take -replace '/', '-'
    foreach ($f in (Join-Path $Logs "$n-her.err"), (Join-Path $Root "$n\state\synaps-avatar\synaps-avatar.log")) {
        if (Test-Path $f) { Select-String -Path $f -Pattern 'tray: ' -SimpleMatch | ForEach-Object { Write-Host "    her log: $($_.Line)" } }
    }
}

<# Her icon out of the overflow. She does it herself, the first time it's in (promote.rs, a few seconds
   after the add: Explorer takes no value written during it): her NotifyIconSettings key (Explorer
   writes one the first time an icon is in) IsPromoted = 1, once. Said here, and a problem if she didn't
   (the take's fresh state folder has no marker); then the key is set from here, as before, and Explorer
   started again if that alone doesn't bring it out (she puts her icon back on TaskbarCreated, and
   Explorer reads the key then). Her button. The 8 s wait is the judge of her own write: when she did it
   and her icon isn't out in 8 s, that's a failure too, with what the shell says of her icon then. #>
function Show-TrayIcon {
    $keys = { @(Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue | Where-Object { [string]$_.GetValue('ExecutablePath') -like '*\synaps-avatar.exe' }) }
    $null = Wait-Until { (& $keys).Count -gt 0 } 20 'her NotifyIconSettings key'
    $g = Get-HerTrayGuid
    $mine = { @(& $keys | Where-Object { $kg = Get-KeyGuid $_; (-not $kg) -or ($kg -eq $g) } | Where-Object { $_.GetValue('IsPromoted') -eq 1 }) }
    $herself = Wait-Until { (& $mine).Count -gt 0 } 20 'her own IsPromoted = 1'
    if ($herself) {
        Write-Host "  she promoted herself: NotifyIconSettings\$((& $mine)[0].PSChildName) (nothing set from here)"
    } else {
        # She didn't: the key is set from here, so the take goes on (and the failure is said).
        $script:Fails.Add("${script:Take}: she didn't promote her own icon (no IsPromoted = 1 in her NotifyIconSettings key after 20 s; set from here)")
        foreach ($k in & $keys) { Write-Host "    key $($k.PSChildName): $($k.GetValue('ExecutablePath')) IconGuid=$($k.GetValue('IconGuid')) UID=$($k.GetValue('UID')) IsPromoted=$($k.GetValue('IsPromoted'))" }
        foreach ($k in & $keys) {
            Set-ItemProperty -Path $k.PSPath -Name IsPromoted -Value 1 -Type DWord
            Write-Host "  promoted from here: NotifyIconSettings\$($k.PSChildName) ($($k.GetValue('InitialTooltip')))"
        }
    }
    if (-not (Wait-Until { Find-TrayButton } 8 'her icon on the taskbar, promoted live')) {
        # Her write alone didn't bring the icon out in 8 s: a failure, and Explorer started again so the take goes on.
        if ($herself) { $script:Fails.Add("${script:Take}: she set IsPromoted = 1 on her own icon and it wasn't on the taskbar 8 s later (Explorer started again from here)") }
        $hr0 = 0
        $r0 = [ShotsTray.Native]::IconRect((Get-HerTrayGuid), [ref]$hr0)
        Write-Host "  not on the taskbar: the shell by her GUID: $(if ($r0) { $r0 -join ',' } else { 'hr 0x{0:x8}' -f $hr0 })"
        Get-TrayButtons | ForEach-Object { Write-Host "    taskbar: '$($_.Name)' ($($_.Class)) at $($_.X),$($_.Y) $($_.W)x$($_.H)" }
        Write-HerTrayLog
        Restart-ShotExplorer
        $null = Wait-Until { Find-TrayButton } 30 'her icon on the taskbar, after Explorer'
    }
    $b = Find-TrayButton
    $hr = 0
    $g = [ShotsTray.Native]::IconRect((Get-HerTrayGuid), [ref]$hr)
    Write-Host "  her icon: UIA $(if ($b) { "'$($b.Name)' at $($b.X),$($b.Y) $($b.W)x$($b.H)" } else { 'none' }); the shell by her GUID: $(if ($g) { $g -join ',' } else { 'hr 0x{0:x8}' -f $hr })"
    if (-not $b) {
        Get-TrayButtons | ForEach-Object { Write-Host "    taskbar: '$($_.Name)' ($($_.Class)) at $($_.X),$($_.Y) $($_.W)x$($_.H)" }
        throw "$script:Take: her icon isn't on the taskbar"
    }
    return $b
}

<# Visible windows that came up since `$Before` (handles), each said. #>
function New-Windows([object[]]$Before) {
    foreach ($w in [ShotsTray.Native]::Of(0) | Where-Object { $_.Handle -notin $Before }) {
        Write-Host "    new window: class $($w.Class), pid $($w.Pid), at $($w.X),$($w.Y) $($w.W)x$($w.H)"
        $w
    }
}

<# Her open menu: a visible #32768 window of hers. #>
function Get-HerMenu {
    [ShotsTray.Native]::Of([uint32]$script:Her.Id) | Where-Object { $_.Class -eq '#32768' } | Select-Object -First 1
}

<# The readers a screen reader reads her menu by, compiled apart (a failure here is said, never the
   set's): MSAA (oleacc's menu proxy: AccessibleObjectFromWindow, a menu window's client, its rows
   its children) and UI Automation's COM client (UIA3, Narrator's: a Win32 menu's rows come to it
   through that MSAA proxy). #>
function Add-ShotReaders {
    if ('ShotsTray.Msaa' -as [type]) { return $true }
    try {
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace ShotsTray {
// IAccessible's first slots after IDispatch's, in its vtable's order.
[ComImport, Guid("618736E0-3C3D-11CF-810C-00AA00389B71"), InterfaceType(ComInterfaceType.InterfaceIsDual)]
public interface IAcc {
    [return: MarshalAs(UnmanagedType.IDispatch)] object Parent();
    int ChildCount();
    [return: MarshalAs(UnmanagedType.IDispatch)] object Child(object id);
    [return: MarshalAs(UnmanagedType.BStr)] string Name(object id);
    [return: MarshalAs(UnmanagedType.BStr)] string Value(object id);
    [return: MarshalAs(UnmanagedType.BStr)] string Description(object id);
    object Role(object id);
}

public static class Msaa {
    [DllImport("oleacc.dll")] static extern int AccessibleObjectFromWindow(IntPtr h, uint id, ref Guid iid, [MarshalAs(UnmanagedType.IUnknown)] out object acc);

    /// Window h's client as MSAA reads it, a line each (indented by depth): self's name, role and
    /// child count, then each simple child's id, name and role. Its HRESULT if there's none.
    public static List<string> Rows(long h) {
        var o = new List<string>();
        var iid = new Guid("618736E0-3C3D-11CF-810C-00AA00389B71");
        object acc;
        int hr = AccessibleObjectFromWindow(new IntPtr(h), 0xFFFFFFFC, ref iid, out acc);
        if (hr != 0 || acc == null) { o.Add("AccessibleObjectFromWindow: 0x" + hr.ToString("x8")); return o; }
        Walk((IAcc)acc, 0, o);
        return o;
    }

    /// The names of window h's simple children (a menu's rows), in order.
    public static List<string> Names(long h) {
        var o = new List<string>();
        var iid = new Guid("618736E0-3C3D-11CF-810C-00AA00389B71");
        object acc;
        if (AccessibleObjectFromWindow(new IntPtr(h), 0xFFFFFFFC, ref iid, out acc) != 0 || acc == null) return o;
        var a = (IAcc)acc;
        int n = 0;
        try { n = a.ChildCount(); } catch { return o; }
        for (int i = 1; i <= n; i++) { string s; try { s = a.Name(i); } catch { s = null; } o.Add(s ?? ""); }
        return o;
    }

    static void Walk(IAcc a, int depth, List<string> o) {
        string pad = new string(' ', depth * 2);
        int n;
        try { n = a.ChildCount(); } catch (Exception e) { o.Add(pad + "children: " + e.Message); return; }
        o.Add(pad + "'" + Safe(() => a.Name(0)) + "' role " + Safe(() => Convert.ToString(a.Role(0))) + ", " + n + " children");
        for (int i = 1; i <= n && o.Count < 80; i++) {
            object child = null;
            try { child = a.Child(i); } catch { }
            var sub = child as IAcc;
            if (sub != null && depth < 3) { Walk(sub, depth + 1, o); continue; }
            o.Add(pad + "  " + i + ": '" + Safe(() => a.Name(i)) + "' role " + Safe(() => Convert.ToString(a.Role(i))));
        }
    }

    static string Safe(Func<string> f) { try { return f() ?? "(null)"; } catch (Exception e) { return "(" + e.Message + ")"; } }
}

// UI Automation's COM client (UIA3: what Narrator reads by; System.Windows.Automation in
// PowerShell 7 has no proxy for a Win32 menu, its rows unseen even in the stock menu). Each
// interface's slots in its vtable's order (the windows crate's), the unused as placeholders.
[ComImport, Guid("352ffba8-0973-437c-a61f-f64cafd81df9"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IUiaCondition {}

[StructLayout(LayoutKind.Sequential)]
public struct UiaPoint { public int X, Y; }

[ComImport, Guid("30cbe57d-d9d0-452a-ab13-7ac5ac4825ee"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IUia {
    void _0(); void _1(); void _2();
    IUiaElement ElementFromHandle(IntPtr hwnd);
    IUiaElement ElementFromPoint(UiaPoint pt);
    IUiaElement GetFocusedElement();
    void _6(); void _7(); void _8(); void _9(); void _10();
    IUiaWalker ControlViewWalker();
    IUiaWalker ContentViewWalker();
    IUiaWalker RawViewWalker();
    void _14(); void _15(); void _16(); void _17();
    IUiaCondition CreateTrueCondition();
}

[ComImport, Guid("d22108aa-8ac5-49a5-837b-37bbb3d7591e"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IUiaElement {
    void _0(); void _1(); void _2();
    IUiaArray FindAll(int scope, IUiaCondition condition);
    void _4(); void _5(); void _6(); void _7(); void _8(); void _9(); void _10(); void _11(); void _12(); void _13(); void _14(); void _15(); void _16();
    int CurrentProcessId();
    int CurrentControlType();
    [return: MarshalAs(UnmanagedType.BStr)] string CurrentLocalizedControlType();
    [return: MarshalAs(UnmanagedType.BStr)] string CurrentName();
    void _21(); void _22(); void _23(); void _24(); void _25(); void _26();
    [return: MarshalAs(UnmanagedType.BStr)] string CurrentClassName();
    void _28(); void _29(); void _30(); void _31(); void _32(); void _33(); void _34(); void _35(); void _36();
    [return: MarshalAs(UnmanagedType.BStr)] string CurrentFrameworkId();
}

[ComImport, Guid("14314595-b4bc-4055-95f2-58f2e42c9855"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IUiaArray {
    int Length();
    IUiaElement GetElement(int i);
}

[ComImport, Guid("4042c624-389c-4afc-a630-9df854a541fc"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IUiaWalker {
    IUiaElement GetParentElement(IUiaElement e);
    IUiaElement GetFirstChildElement(IUiaElement e);
    IUiaElement GetLastChildElement(IUiaElement e);
    IUiaElement GetNextSiblingElement(IUiaElement e);
}

public static class Uia3 {
    static IUia u;
    static IUia U() {
        if (u == null) u = (IUia)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("ff48dba4-60ef-4201-aa87-54103eef594e")));
        return u;
    }

    static string Kind(int id) {
        switch (id) {
            case 50009: return "Menu";
            case 50010: return "MenuBar";
            case 50011: return "MenuItem";
            case 50033: return "Pane";
            case 50038: return "Separator";
            case 50032: return "Window";
            default: return id.ToString();
        }
    }

    /// An element, said: its control type, class, framework, name.
    public static string Say(IUiaElement e) {
        if (e == null) return "(none)";
        return Safe(() => Kind(e.CurrentControlType())) + " '" + Safe(e.CurrentClassName) + "' fw=" + Safe(e.CurrentFrameworkId) + " '" + Safe(e.CurrentName) + "'";
    }

    /// Window h's element and its raw tree three deep, a line each (indented by depth).
    public static List<string> Tree(long h) {
        var o = new List<string>();
        var e = U().ElementFromHandle(new IntPtr(h));
        o.Add(Say(e));
        Walk(U().RawViewWalker(), e, 1, o);
        return o;
    }

    static void Walk(IUiaWalker w, IUiaElement e, int d, List<string> o) {
        var k = w.GetFirstChildElement(e);
        while (k != null && o.Count < 80) {
            o.Add(new string(' ', d * 2) + Say(k));
            if (d < 3) Walk(w, k, d + 1, o);
            k = w.GetNextSiblingElement(k);
        }
    }

    /// The names of window h's descendants (FindAll, every one), in order.
    public static List<string> Names(long h) {
        var o = new List<string>();
        var all = U().ElementFromHandle(new IntPtr(h)).FindAll(4, U().CreateTrueCondition());
        if (all == null) return o;
        for (int i = 0; i < all.Length(); i++) { string s; try { s = all.GetElement(i).CurrentName(); } catch { s = null; } o.Add(s ?? ""); }
        return o;
    }

    /// The element at screen point x, y (physical px; the caller per-monitor aware), and its name.
    public static string[] At(int x, int y) {
        var e = U().ElementFromPoint(new UiaPoint { X = x, Y = y });
        string n = null;
        if (e != null) { try { n = e.CurrentName(); } catch { } }
        return new[] { Say(e), n ?? "" };
    }

    /// The focused element and its parent, said.
    public static string Focus() {
        var f = U().GetFocusedElement();
        return Say(f) + ", under " + (f == null ? "(none)" : Say(U().RawViewWalker().GetParentElement(f)));
    }

    static string Safe(Func<string> f) { try { return f() ?? "(null)"; } catch (Exception e) { return "(" + e.Message + ")"; } }
}
}
'@
        return $true
    }
    catch { Write-Host "  readers: not compiled: $($_.Exception.Message)"; return $false }
}

<# A UI Automation element, said: its control type, class, framework, name. #>
function Format-Uia($e) {
    if (-not $e) { return '(none)' }
    try { $c = $e.Current; "$($c.ControlType.ProgrammaticName) '$($c.ClassName)' fw=$($c.FrameworkId) '$($c.Name)'" }
    catch { "($($_.Exception.Message))" }
}

<# What a screen reader can read in menu window $Win (a [ShotsTray.Native]::Of window), said: MSAA's
   menu proxy (oleacc: its client, each row), UI Automation's COM client (UIA3: the element, its raw
   tree three deep, its descendants' names, what's under points down its middle) and, for the
   record, PowerShell's own System.Windows.Automation (UIA2). On the runner neither UI Automation
   sees a Win32 menu's rows, main's stock menu's included (#112, round 2): a #32768 is a bare Pane,
   no children. Returns the names read, @{ Msaa = ...; Uia3 = ... }. #>
function Get-MenuNames($Win, [string]$What) {
    $Handle = [long]$Win.Handle
    $uia3 = @(); $msaa = @()
    if (Add-ShotReaders) {
        try {
            foreach ($l in [ShotsTray.Uia3]::Tree($Handle)) { Write-Host "    uia3 $l" }
            $uia3 = @([ShotsTray.Uia3]::Names($Handle) | Where-Object { $_ })
        }
        catch { Write-Host "  uia3 ($What): $($_.Exception.Message)" }
        try {
            [ShootWin.Native]::Aware()
            $x = [int]($Win.X + $Win.W / 2)
            foreach ($k in 0..7) {
                $y = [int]($Win.Y + $Win.H * (2 * $k + 1) / 16)
                $at = [ShotsTray.Uia3]::At($x, $y)
                Write-Host "    uia3 at $x,$y : $($at[0])"
                if ($at[1] -and $at[1] -cnotin $uia3) { $uia3 += $at[1] }
            }
        }
        catch { Write-Host "  uia3 ($What) by point: $($_.Exception.Message)" }
        try {
            foreach ($l in [ShotsTray.Msaa]::Rows($Handle)) { Write-Host "    msaa $l" }
            $msaa = @([ShotsTray.Msaa]::Names($Handle) | Where-Object { $_ })
        }
        catch { Write-Host "  msaa ($What): $($_.Exception.Message)" }
    }
    Write-Host "  uia3 ($What): $($uia3 -join ' | ')"
    Write-Host "  msaa ($What): $($msaa -join ' | ')"
    try {
        if (-not ('System.Windows.Automation.AutomationElement' -as [type])) { Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes }
        $el = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Handle)
        $rows = @($el.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))
        Write-Host "  uia2 ($What): $(Format-Uia $el) rows=$($rows.Count)"
    }
    catch { Write-Host "  uia2 ($What): $($_.Exception.Message)" }
    @{ Uia3 = $uia3; Msaa = $msaa }
}

<# UI Automation's focused element (and its parent), said (UIA3's; UIA2's for the record). #>
function Show-UiaFocus([string]$When) {
    if ((Add-ShotReaders)) {
        try { Write-Host "  uia3 focus ($When): $([ShotsTray.Uia3]::Focus())" }
        catch { Write-Host "  uia3 focus ($When): $($_.Exception.Message)" }
    }
    if (-not ('System.Windows.Automation.AutomationElement' -as [type])) { Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes }
    try { Write-Host "  uia2 focus ($When): $(Format-Uia ([System.Windows.Automation.AutomationElement]::FocusedElement))" }
    catch { Write-Host "  uia2 focus ($When): $($_.Exception.Message)" }
}

<# A shot of the taskbar's corner: her icon and -Also (her menu, the tooltip), padded (-Above px
   over them, at 100%), down to the screen's bottom; at least -Aspect times as wide as tall (0: as
   it comes). Its right edge is the screen's, or short of anything on the taskbar that tells the
   time or date (if the clock stayed). Never off the screen. #>
function Get-CornerRect {
    param([Parameter(Mandatory)]$Icon, [object[]]$Also = @(), [double]$Aspect = 0, [int]$Above = 24)
    $sf = [double](State).scale_factor
    $pad = [int](24 * $sf)
    $sc = [ShootWin.Native]::Screen()
    $all = @($Icon) + @($Also)
    $l = ($all | ForEach-Object { $_.X } | Measure-Object -Minimum).Minimum - $pad
    $t = ($all | ForEach-Object { $_.Y } | Measure-Object -Minimum).Minimum - [int]($Above * $sf)
    $b = $sc[1] + $sc[3]
    $r = $sc[0] + $sc[2]
    $clock = @(Get-TrayButtons | Where-Object { $_.X -ge $Icon.X + $Icon.W - 1 -and $_.Name -match '\d{1,2}:\d{2}|\d{1,2}/\d{1,2}/\d{2,4}|\d{4}-\d{2}-\d{2}' })
    if ($clock) {
        $r = ($clock | ForEach-Object { $_.X } | Measure-Object -Minimum).Minimum - [int](2 * $sf)
        Write-Warning "${script:Take}: the clock stayed on the taskbar ('$($clock[0].Name)'): the shot ends at x $r"
    }
    $r = [Math]::Min([Math]::Max($r, ($all | ForEach-Object { $_.X + $_.W } | Measure-Object -Maximum).Maximum), $sc[0] + $sc[2])
    if ($Aspect -gt 0) { $l = [Math]::Min($l, $r - [int]($Aspect * ($b - $t))) }
    $l = [Math]::Max($l, $sc[0]); $t = [Math]::Max($t, $sc[1])
    return @([int]$l, [int]$t, [int]($r - $l), [int]($b - $t))
}

# ---- run ---------------------------------------------------------------------------------------

# The tray set last: its taskbar (dark, no clock, Explorer restarted) stays for whatever runs after.
$Sets = @($Sets | Where-Object { $_ -ne 'tray' }) + @($Sets | Where-Object { $_ -eq 'tray' })
Clear-ShotDesktop

$taken = [System.Collections.Generic.HashSet[string]]::new()
foreach ($set in $Sets) {
    if (-not $Scenes.Contains($set)) { throw "no scene set '$set' (sets: $($Scenes.PSBase.Keys -join ', '))" }
    # PSBase: a scene named like a hashtable's property (keys, values, count) would shadow it.
    foreach ($name in @($Scenes[$set].PSBase.Keys)) {
        if ($Only.Count -and "$set/$name" -notin $Only) { continue }
        $null = $taken.Add("$set/$name")
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
# An -Only take that never ran (no such scene, or one the loop missed) is a problem, not a pass.
foreach ($o in $Only) {
    if ($o.Split('/')[0] -in $Sets -and -not $taken.Contains($o)) { $script:Fails.Add("${o}: no such take, never shot") }
}

[pscustomobject]@{ index = $script:Index; fails = $script:Fails; screen = [ShootWin.Native]::Screen(); scales = [ShootWin.Native]::Scales() } |
    ConvertTo-Json -Depth 8 | Set-Content -Path (Join-Path $Out 'index.json') -Encoding utf8NoBOM
Write-Host "$($script:Index.Count) shots, $($script:Fails.Count) problem(s)"
$script:Fails | ForEach-Object { Write-Warning $_ }
# The step fails with any problem (the index and the shots are still kept: the upload runs anyway).
if ($script:Index.Count -eq 0) { throw 'no shots at all' }
if ($script:Fails.Count) { exit 1 }
