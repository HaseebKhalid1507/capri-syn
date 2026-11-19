# The licences of the Rust crates compiled into her binary, for one target: cargo-about (pinned below,
# its release tarball's SHA-256 checked) with about.toml and about.hbs beside this script. Apache-2.0
# (GPUI, compiled in) asks for its text with every object-form copy: this file carries it, and every
# other crate's licence.
#
#   pwsh installer/licenses/third-party.ps1 -Target x86_64-unknown-linux-gnu -Out FILE
#
# Runs where cargo is (cargo-about reads `cargo metadata` and the crates' own licence files): CI, or a
# machine that builds her. release.yml's linux job calls it for the tarball; packaging/windows/build.ps1
# for her installer. cargo-about is kept in target/cargo-about-<version>.
param(
    [Parameter(Mandatory)] [string]$Target,
    [Parameter(Mandatory)] [string]$Out
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

$version = '0.9.3'
$onWindows = $env:OS -eq 'Windows_NT'
$hostTriple = if ($onWindows) { 'x86_64-pc-windows-msvc' } else { 'x86_64-unknown-linux-musl' }
$pins = @{
    'x86_64-pc-windows-msvc'    = '9e2942533ef1c592c4a70bdb384cae1550a3f5fc9ae40379b6662e3d2f16fe02'
    'x86_64-unknown-linux-musl' = '3ad1e94ea1e63d69d1961f4fb15ccbbe9575d906111f96d2cb4dce6df48f71b4'
}
$name = "cargo-about-$version-$hostTriple"
$dir = Join-Path $root "target/cargo-about-$version"
$bin = Join-Path $dir "$name/$(if ($onWindows) { 'cargo-about.exe' } else { 'cargo-about' })"
if (-not (Test-Path $bin)) {
    New-Item -ItemType Directory -Force $dir | Out-Null
    $tgz = Join-Path $dir "$name.tar.gz"
    Invoke-WebRequest -UseBasicParsing "https://github.com/EmbarkStudios/cargo-about/releases/download/$version/$name.tar.gz" -OutFile $tgz
    $got = (Get-FileHash -Algorithm SHA256 $tgz).Hash.ToLowerInvariant()
    if ($got -ne $pins[$hostTriple]) { throw "$name.tar.gz: SHA-256 $got, pinned $($pins[$hostTriple])" }
    # Windows' own tar (bsdtar): Git's GNU tar reads a drive letter as a remote host.
    $tar = if ($onWindows) { Join-Path $env:SystemRoot 'System32\tar.exe' } else { 'tar' }
    & $tar -xzf $tgz -C $dir
    if ($LASTEXITCODE) { throw "tar failed ($LASTEXITCODE)" }
    Remove-Item $tgz
}

$outDir = Split-Path -Parent ([IO.Path]::GetFullPath($Out))
New-Item -ItemType Directory -Force $outDir | Out-Null
$body = [IO.Path]::GetTempFileName()
Push-Location $root
try {
    & $bin generate --locked --fail --target $Target -c (Join-Path $PSScriptRoot 'about.toml') -o $body (Join-Path $PSScriptRoot 'about.hbs')
    if ($LASTEXITCODE) { Remove-Item -Force $body; throw "cargo-about failed ($LASTEXITCODE)" }
} finally { Pop-Location }

# What it must hold: GPUI's platform crate for this target, under Apache-2.0, and its text.
$text = [IO.File]::ReadAllText($body)
$gpui = if ($Target -like '*windows*') { 'gpui-pre-windows' } else { 'gpui-pre-linux' }
foreach ($must in @($gpui, 'gpui-pre ', 'Apache License', 'Version 2.0, January 2004')) {
    if (-not $text.Contains($must)) { throw "the third-party licences for $Target have no '$must'" }
}
$crates = ([regex]::Matches($text, '(?m)^  \S+ \d+\.\d+\S*')).Count
$lock = (Get-FileHash -Algorithm SHA256 (Join-Path $root 'Cargo.lock')).Hash.ToLowerInvariant().Substring(0, 12)
$head = "synaps-avatar, ${Target}: third-party licences (cargo-about $version, Cargo.lock sha256 $lock)`n`n"
[IO.File]::WriteAllText([IO.Path]::GetFullPath($Out), $head + $text, (New-Object Text.UTF8Encoding $false))
Remove-Item -Force $body
Write-Output "third-party licences: $Out ($crates crate entries, $([math]::Round((Get-Item $Out).Length / 1KB)) KB)"
