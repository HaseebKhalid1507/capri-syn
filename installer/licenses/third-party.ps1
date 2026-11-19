# The licences of the Rust crates compiled into her binary, for one target: cargo-about (pinned below,
# its release tarball's SHA-256 checked) with about.toml and about.hbs beside this script. Apache-2.0
# (GPUI, compiled in) asks for its text with every object-form copy: this file carries it, and every
# other crate's licence.
#
#   pwsh installer/licenses/third-party.ps1 -Target x86_64-unknown-linux-gnu -Out FILE
#
# Runs where cargo is (cargo-about reads `cargo metadata` and the crates' own licence files): CI, or a
# machine that builds her. release.yml's linux job calls it for the tarball; packaging/windows/build.ps1
# for her installer; ci.yml's check job on every PR (Linux), so a new licence fails there, not on a tag.
# cargo-about's tarball is kept in target/cargo-about-<version>, its SHA-256 checked on every run (a
# cached copy is trusted no more than a download) and unpacked fresh into a temp folder.
param(
    [Parameter(Mandatory)] [string]$Target,
    [Parameter(Mandatory)] [string]$Out
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
. (Join-Path $PSScriptRoot 'check.ps1')   # Get-ThirdPartySection, Test-GpuiUnderApache: the tests read it the same way

$version = '0.9.3'
$onWindows = $env:OS -eq 'Windows_NT'
$hostTriple = if ($onWindows) { 'x86_64-pc-windows-msvc' } else { 'x86_64-unknown-linux-musl' }
$pins = @{
    'x86_64-pc-windows-msvc'    = '9e2942533ef1c592c4a70bdb384cae1550a3f5fc9ae40379b6662e3d2f16fe02'
    'x86_64-unknown-linux-musl' = '3ad1e94ea1e63d69d1961f4fb15ccbbe9575d906111f96d2cb4dce6df48f71b4'
}
$name = "cargo-about-$version-$hostTriple"
$dir = Join-Path $root "target/cargo-about-$version"
$tgz = Join-Path $dir "$name.tar.gz"
if (-not (Test-Path $tgz)) {
    New-Item -ItemType Directory -Force $dir | Out-Null
    Invoke-WebRequest -UseBasicParsing "https://github.com/EmbarkStudios/cargo-about/releases/download/$version/$name.tar.gz" -OutFile $tgz
}
$got = (Get-FileHash -Algorithm SHA256 $tgz).Hash.ToLowerInvariant()
if ($got -ne $pins[$hostTriple]) { Remove-Item -Force $tgz; throw "$name.tar.gz: SHA-256 $got, pinned $($pins[$hostTriple])" }
$tool = Join-Path ([IO.Path]::GetTempPath()) "cargo-about-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory $tool | Out-Null
# Windows' own tar (bsdtar): Git's GNU tar reads a drive letter as a remote host.
$tar = if ($onWindows) { Join-Path $env:SystemRoot 'System32\tar.exe' } else { 'tar' }
& $tar -xzf $tgz -C $tool
if ($LASTEXITCODE) { Remove-Item -Recurse -Force $tool; throw "tar failed ($LASTEXITCODE)" }
$bin = Join-Path $tool "$name/$(if ($onWindows) { 'cargo-about.exe' } else { 'cargo-about' })"

$outDir = Split-Path -Parent ([IO.Path]::GetFullPath($Out))
New-Item -ItemType Directory -Force $outDir | Out-Null
$body = [IO.Path]::GetTempFileName()
Push-Location $root
try {
    & $bin generate --locked --fail --target $Target -c (Join-Path $PSScriptRoot 'about.toml') -o $body (Join-Path $PSScriptRoot 'about.hbs')
    if ($LASTEXITCODE) { Remove-Item -Force $body; throw "cargo-about failed ($LASTEXITCODE)" }
} finally { Pop-Location; Remove-Item -Recurse -Force $tool }
$text = [IO.File]::ReadAllText($body)
Remove-Item -Force $body
$sections = @(Get-ThirdPartySection $text)
$crates = @($sections | ForEach-Object { $_.Crates })

# GPUI's platform crate for this target (patched): in an Apache-2.0 section, with the licence's text.
$gpui = if ($Target -like '*windows*') { 'gpui-pre-windows' } else { 'gpui-pre-linux' }
if (-not (Test-GpuiUnderApache $sections $gpui)) { throw "the third-party licences for $Target don't list $gpui under Apache-2.0's text" }

# What a crate embeds without declaring it, kept beside this script (installer/licenses/list): written
# for one version each. gpui-kit-assets embeds Lucide's icons (ISC; vendored/lucide-LICENSE).
foreach ($pin in @('gpui-kit-assets 0.9.1')) {
    if ($crates -notcontains $pin) { throw "the third-party licences for ${Target}: '$pin' changed or left: check what it embeds now, then refresh installer/licenses/vendored/ and this pin" }
}
# She highlights no code (Cargo.toml: gpui-kit without "syntect"). syntect and two-face embed syntax
# definitions under their own licences (two-face's acknowledgements), and gpui-component's tree-sitter
# feature compiles in query files from other repositories; none of those licences are here.
$back = @($crates | Where-Object { $_ -match '^(syntect|two-face|tree-sitter[-\w]*) ' })
if ($back) { throw "the third-party licences for ${Target}: $($back -join ', ') came back; what they embed (syntax definitions, queries) carries licences that aren't shipped (two-face's acknowledgements_full.md)" }

# Crates whose licence text is SPDX's template, with no copyright line (`<year> <copyright holders>`):
# cargo-about found no licence file of theirs it could match. Linux's list, plus whatever Windows turns
# out to need (nothing today: windows-rs is clarified in about.toml); a new one stops the build. Give
# it a [crate.clarify] in about.toml (its licence file, from its repository) or, after looking, add it
# here. Work the list down, never up without a reason.
$noCopyrightLine = @(
    'accesskit', 'accesskit_atspi_common', 'accesskit_consumer', 'accesskit_unix', 'accesskit_windows',
    'exr', 'granit-parser', 'harfrust', 'libm', 'lyon', 'lyon_algorithms', 'lyon_geom', 'lyon_path',
    'lyon_tessellation', 'mac', 'pathfinder_geometry', 'pathfinder_simd', 'profiling',
    'profiling-procmacros', 'rust-i18n-macro', 'rust-i18n-support', 'seahash', 'sha1_smol', 'svg_fmt',
    'taffy', 'wayland-protocols-plasma', 'xim-ctext', 'xim-parser', 'zune-core', 'zune-inflate',
    'zune-jpeg'
)
$template = @($sections | Where-Object { $_.Text -match '<year>|<copyright holders>|<owner>' } | ForEach-Object { $_.Crates } | ForEach-Object { ($_ -split ' ')[0] } | Sort-Object -Unique)
$new = @($template | Where-Object { $noCopyrightLine -notcontains $_ })
if ($new) { throw "the third-party licences for ${Target}: $($new -join ', ') get SPDX's template text, no copyright line: clarify it in about.toml, or name it in `$noCopyrightLine after looking" }
$lock = (Get-FileHash -Algorithm SHA256 (Join-Path $root 'Cargo.lock')).Hash.ToLowerInvariant().Substring(0, 12)
$head = "synaps-avatar, ${Target}: third-party licences (cargo-about $version, Cargo.lock sha256 $lock)`n`n"
[IO.File]::WriteAllText([IO.Path]::GetFullPath($Out), $head + $text, (New-Object Text.UTF8Encoding $false))
Write-Output "third-party licences: $Out ($($crates.Count) crate entries under $($sections.Count) licence texts; $($template.Count) without a copyright line of their own; $([math]::Round((Get-Item $Out).Length / 1KB)) KB)"
