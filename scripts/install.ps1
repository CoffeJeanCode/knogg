# knogg installer for Windows — downloads the latest release from GitHub.
# Usage: irm https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.ps1 | iex
#
# Optional env vars (set before running):
#   $env:KNOGG_INSTALL_DIR — install directory (default: $env:LOCALAPPDATA\Programs\knogg)
#   $env:KNOGG_VERSION     — specific version tag, e.g. "v1.2.0" (default: latest)
#   $env:GITHUB_TOKEN      — GitHub PAT to avoid API rate limits

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Repo       = "CoffeJeanCode/knogg"
$BinaryName = "knogg.exe"
$InstallDir = if ($env:KNOGG_INSTALL_DIR) { $env:KNOGG_INSTALL_DIR } else { "$env:LOCALAPPDATA\Programs\knogg" }
$ApiBase    = "https://api.github.com/repos/$Repo"

function Write-Info  { Write-Host "==> $args" -ForegroundColor Cyan }
function Write-Warn  { Write-Host "!>  $args" -ForegroundColor Yellow }
function Write-Fatal { Write-Host "!!> $args" -ForegroundColor Red; exit 1 }

function Invoke-Api([string]$Url) {
    $headers = @{ 'User-Agent' = 'knogg-installer' }
    if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    try {
        return Invoke-RestMethod -Uri $Url -Headers $headers -UseBasicParsing
    } catch {
        Write-Fatal "GitHub API request failed: $_`nIf rate-limited, set `$env:GITHUB_TOKEN and retry."
    }
}

function Find-WindowsAsset([array]$Assets) {
    # Try exact name first, then progressively looser patterns
    $patterns = @(
        'knogg-windows-amd64.exe',
        'knogg-windows*.exe',
        '*windows*amd64*.exe',
        '*windows*.exe',
        'knogg*.exe'
    )
    foreach ($pat in $patterns) {
        $match = $Assets | Where-Object { $_.name -like $pat } | Select-Object -First 1
        if ($match) { return $match }
    }
    return $null
}

# Resolve release
if ($env:KNOGG_VERSION) {
    Write-Info "fetching release $($env:KNOGG_VERSION)..."
    $Release = Invoke-Api "$ApiBase/releases/tags/$($env:KNOGG_VERSION)"
} else {
    Write-Info "fetching latest release..."
    $Release = Invoke-Api "$ApiBase/releases/latest"
}

$Tag   = $Release.tag_name
$Asset = Find-WindowsAsset $Release.assets

if (-not $Asset) {
    $available = ($Release.assets | ForEach-Object { "  - $($_.name)" }) -join "`n"
    Write-Host ""
    Write-Warn "No Windows binary found in release $Tag."
    Write-Host "Available assets:" -ForegroundColor Yellow
    Write-Host $available
    Write-Host ""
    Write-Host "Build from source or wait for the next release." -ForegroundColor Yellow
    exit 1
}

$DownloadUrl = $Asset.browser_download_url

Write-Info "version : $Tag"
Write-Info "asset   : $($Asset.name)"
Write-Info "install : $InstallDir\$BinaryName"

# Create install directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Download to temp file
$TmpFile = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), "knogg-$Tag.exe")
Write-Info "downloading..."
try {
    $headers = @{ 'User-Agent' = 'knogg-installer' }
    if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $TmpFile -Headers $headers -UseBasicParsing
} catch {
    Write-Fatal "download failed: $_"
}

# Verify the downloaded file is a valid PE binary (not an HTML error page)
$magic = [System.IO.File]::ReadAllBytes($TmpFile)[0..1]
if (-not ($magic[0] -eq 0x4D -and $magic[1] -eq 0x5A)) {
    Remove-Item $TmpFile -Force
    Write-Fatal "downloaded file is not a Windows binary (got HTML?). Check the release asset."
}

# Install
$Dest = Join-Path $InstallDir $BinaryName
Move-Item -Force $TmpFile $Dest

# Verify binary runs
try {
    $Version = & $Dest --version 2>&1
    Write-Info "installed: $Version"
} catch {
    Write-Warn "binary placed at $Dest but --version failed (may need a terminal restart)"
}

# Add to user PATH if not already present
$UserPath = [System.Environment]::GetEnvironmentVariable("Path", "User") ?? ""
if ($UserPath -notlike "*$InstallDir*") {
    [System.Environment]::SetEnvironmentVariable(
        "Path",
        ($UserPath.TrimEnd(';') + ";$InstallDir"),
        "User"
    )
    Write-Info "added $InstallDir to your PATH"
    Write-Warn "restart your terminal (or run: `$env:PATH += ';$InstallDir') to use knogg now"
} else {
    Write-Info "$InstallDir already in PATH"
}

Write-Host ""
Write-Host "Quick start:" -ForegroundColor Green
Write-Host "  cd your-project"
Write-Host "  knogg init"
Write-Host "  knogg status"
