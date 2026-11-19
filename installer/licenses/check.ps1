# Checks her Windows install carries her licence and every notice that goes with her binary
# (installer/licenses/list), each the same bytes as the repo's, and the crates' licences generated for
# that build (GPUI's platform crate among them, under Apache-2.0's text). Dot-sourced by the tests
# that install her (packaging/windows/test-install.ps1, installer/test-windows.ps1); it reports
# through their own `Check`.
#
#   . (Join-Path $repo 'installer\licenses\check.ps1'); Test-HerLicence -App $app
#
# Get-ThirdPartySection reads a THIRD-PARTY-LICENSES.txt (about.hbs's layout): one object per licence
# text, its heading, the crates under it ("name version") and the text. third-party.ps1 checks the
# file it makes with it, the tests the one installed: the same reading, the same counts.
function Get-ThirdPartySection([string]$Text) {
    $rule = '=' * 80
    $dash = '-' * 80
    foreach ($m in [regex]::Matches($Text, "(?ms)^$rule\r?\n(?<head>[^\r\n]*), used by:\r?\n(?<crates>.*?)^$dash\r?\n(?<text>.*?)(?=^$rule\r?\n|\z)")) {
        [pscustomobject]@{
            Licence = $m.Groups['head'].Value
            Crates  = @([regex]::Matches($m.Groups['crates'].Value, '(?m)^  (\S+) (\S+)') | ForEach-Object { "$($_.Groups[1].Value) $($_.Groups[2].Value)" })
            Text    = $m.Groups['text'].Value
        }
    }
}

# GPUI's platform crate is Apache-2.0 and patched: it must sit in an Apache-2.0 section that holds the
# licence's full text, not merely appear somewhere in the file.
function Test-GpuiUnderApache([object[]]$Sections, [string]$Gpui) {
    @($Sections | Where-Object { $_.Licence -like '*(Apache-2.0)' -and $_.Text.Contains('Version 2.0, January 2004') -and @($_.Crates | Where-Object { $_ -like "$Gpui *" }).Count }).Count -gt 0
}

function Test-HerLicence([string]$App) {
    $repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    foreach ($line in Get-Content (Join-Path $repo 'installer\licenses\list')) {
        if ($line -match '^\s*(#|$)') { continue }
        $from, $to = $line -split "`t", 2
        $got = Join-Path $App $to
        if ($from -eq '@third-party@') {
            $sections = @(Get-ThirdPartySection $(if (Test-Path $got) { [IO.File]::ReadAllText($got) } else { '' }))
            $entries = ($sections | ForEach-Object { $_.Crates.Count } | Measure-Object -Sum).Sum
            Check (Test-GpuiUnderApache $sections 'gpui-pre-windows') "$to in $App`: gpui-pre-windows under Apache-2.0's text ($entries crate entries)"
            continue
        }
        $from = $from -replace '@os@', 'windows'
        $want = (Get-FileHash -Algorithm SHA256 (Join-Path $repo $from)).Hash
        Check ((Test-Path $got) -and (Get-FileHash -Algorithm SHA256 $got).Hash -eq $want) "$to in $App, the repo's $from"
    }
}
