<#
.SYNOPSIS
  Her shots on a Windows runner (.github/workflows/shots.yml). Per take: a fake Synaps daemon
  (tools/fakedaemon.py, so she isn't OFFLINE; FAKE: no model), the fake voice sidecar where a take
  wants voice (tools/fakevoice.py), her own settings.toml and folders; she is driven through her
  control socket, and keys or clicks (tools/shoot-win.ps1) only where it can't. Each step is shot
  cropped to her windows (her, the Line, the bubble, the menu), its `state` beside it.

  pwsh tools/shots/scenes.ps1 -Exe her.exe -Pack packs\capri -Out shots\100 [-Sets settings,header] [-Only settings/on,header/hf-470]

  Sets:
    settings  her Settings page (#77): chat, ctrl+, then right, down, esc; her menu; voice off; brain
              off; [brain] enabled = false by a reload; CAN'T SAVE on a read-only settings.toml; the
              cog and `CHAT` (their rects, from `state`) with an empty chat and a full one.
    header    the bubble's header (#78) at 470 px and at 340 px: hands-free and a reply, a prompt
              with 3 waiting, his long words (YOU) while the turn runs.
  Out: <Out>\<set>\<take>\<step>.png + .json (her `state`), <Out>\index.json, <Out>\logs\.
#>
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$Pack,
    [Parameter(Mandatory)][string]$Out,
    [string[]]$Sets = @('settings', 'header'),
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
        [string]$Settings = '', [hashtable]$Env = @{}, [object[]]$History, [switch]$Voice, [int]$DeltaMs = 30, [string[]]$HerArgs = @()
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
    foreach ($k in $Env.Keys) { $herEnv[$k] = $Env[$k] }
    Set-TakeEnv $herEnv

    $argv = @('--pack', (Q $Pack), '--socket', (Q (Join-Path $t 'fake\d.sock'))) + @($HerArgs | Where-Object { $_ })
    $script:Her = Start-Process $Exe -ArgumentList $argv -NoNewWindow -PassThru `
        -RedirectStandardOutput "$log-her.log" -RedirectStandardError "$log-her.err"
    $up = Wait-Until { (State).loaded -eq $true } 120 "her frames ($script:Take)"
    if (-not $up) { throw "$script:Take: she never loaded (see $log-her.err)" }
    $live = Wait-Until { (State).session.live -eq $true } 30 "her session ($script:Take)"
    if (-not $live) { $script:Fails.Add("$script:Take: OFFLINE (the fake daemon never answered)") }
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

<# A step's shot: her windows, cropped; her `state` beside it. #>
function Shot([string]$Step, [string]$Note = '') {
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
            Key 'right' 1400; Shot '3-change' 'then right'
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
            Key 'ctrl+comma' 1400; Key 'down'; Key 'right' 1400
            Shot '7-locked' 'voice off in settings.toml: ctrl+, down right'
        }
        nobrain  = {
            Start-Take settings nobrain -Env @{ SA_NO_BRAIN = '1' } -History $Short
            $null = Fake "say $Reply"
            Start-Sleep 4
            $null = Ctl 'talk tap'
            Start-Sleep 2
            Focus-Line
            Key 'ctrl+comma' 1400; Key 'right' 1400
            Shot '8-nobrain' 'brain off: ctrl+, right'
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
