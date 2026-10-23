# Install multi-herdr from a GitHub release. No Rust toolchain needed.
#   $env:HORCH_VERSION      tag to install, for example v0.2.0 (default: latest)
#   $env:HORCH_INSTALL_DIR  directory for the binaries (default: horch's own default)
$ErrorActionPreference = 'Stop'

$repo = 'MattMakes/multi-herdr'
$target = 'x86_64-pc-windows-msvc'

$tag = $env:HORCH_VERSION
if (-not $tag) {
    $tag = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
}
if (-not $tag) { throw 'install.ps1: could not find the latest release tag' }

$name = "multi-herdr-$tag-$target.zip"
$base = "https://github.com/$repo/releases/download/$tag"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "Downloading $name"
    Invoke-WebRequest -UseBasicParsing "$base/$name" -OutFile (Join-Path $tmp $name)
    Invoke-WebRequest -UseBasicParsing "$base/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')

    $line = Get-Content (Join-Path $tmp 'SHA256SUMS') |
        Where-Object { $_ -match "^[0-9a-fA-F]{64}\s+$([regex]::Escape($name))$" } |
        Select-Object -First 1
    if (-not $line) { throw "install.ps1: $name is not listed in SHA256SUMS" }
    $expected = ($line -split '\s+')[0].ToLower()
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $name)).Hash.ToLower()
    if ($expected -ne $actual) { throw "install.ps1: checksum mismatch for $name" }

    Expand-Archive -Path (Join-Path $tmp $name) -DestinationPath $tmp -Force

    Push-Location $tmp
    try {
        if ($env:HORCH_INSTALL_DIR) {
            .\horch.exe install --dir $env:HORCH_INSTALL_DIR
        } else {
            .\horch.exe install
        }
        if ($LASTEXITCODE -ne 0) { throw 'install.ps1: horch install failed' }

        if (-not (Get-Command herdr -ErrorAction SilentlyContinue)) {
            Write-Host 'herdr is not on PATH: running herdr-install'
            .\herdr-install.exe
        }
    } finally {
        Pop-Location
    }

    Write-Host 'Done. Run: horch doctor'
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
