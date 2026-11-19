# Checks her Windows install carries her licence and every notice that goes with her binary
# (installer/licenses/list), each the same bytes as the repo's, and the crates' licences generated for
# that build (GPUI's platform crate among them, under Apache-2.0's text). Dot-sourced by the tests
# that install her (packaging/windows/test-install.ps1, installer/test-windows.ps1); it reports
# through their own `Check`.
#
#   . (Join-Path $repo 'installer\licenses\check.ps1'); Test-HerLicence -App $app
function Test-HerLicence([string]$App) {
    $repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    foreach ($line in Get-Content (Join-Path $repo 'installer\licenses\list')) {
        if ($line -match '^\s*(#|$)') { continue }
        $from, $to = $line -split "`t", 2
        $got = Join-Path $App $to
        if ($from -eq '@third-party@') {
            $t = if (Test-Path $got) { [IO.File]::ReadAllText($got) } else { '' }
            Check ($t -match '(?m)^  gpui-pre-windows ' -and $t.Contains('Version 2.0, January 2004')) "$to in $App`: gpui-pre-windows under Apache-2.0's text ($([regex]::Matches($t, '(?m)^  \S+ \d').Count) crate entries)"
            continue
        }
        $from = $from -replace '@os@', 'windows'
        $want = (Get-FileHash -Algorithm SHA256 (Join-Path $repo $from)).Hash
        Check ((Test-Path $got) -and (Get-FileHash -Algorithm SHA256 $got).Hash -eq $want) "$to in $App, the repo's $from"
    }
}
