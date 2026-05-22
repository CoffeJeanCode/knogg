# knogg installer for Windows — downloads the latest release from GitHub.
# Usage: irm https://raw.githubusercontent.com/CoffeJeanCode/knogg/main/scripts/install.ps1 | iex
#
# Optional env vars (set before running):
#   $env:KNOGG_INSTALL_DIR — install directory (default: $env:LOCALAPPDATA\Programs\knogg)
#   $env:KNOGG_VERSION     — specific version tag, e.g. "v1.2.0" (default: latest)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Repo        = "CoffeJeanCode/knogg"
$AssetName   = "knogg-windows-amd64.exe"
$BinaryName  = "knogg.exe"
$InstallDir  = if ($env:KNOGG_INSTALL_DIR) { $env:KNOGG_INSTALL_DIR } else { "$env:LOCALAPPDATA\Programs\knogg" }
$GithubApi   = "https://api.github.com/repos/$Repo"

function Write-Info  { Write-Host "==> $args" -ForegroundColor Cyan }
function Write-Warn  { Write-Host "!>  $args" -ForegroundColor Yellow }
function Write-Fatal { Write-Host "!!> $args" -ForegroundColor Red; exit 1 }

# Resolve download URL
if ($env:KNOGG_VERSION) {
    $Tag         = $env:KNOGG_VERSION
    $DownloadUrl = "https://github.com/$Repo/releases/download/$Tag/$AssetName"
    Write-Info "pinned version: $Tag"
} else {
    Write-Info "fetching latest release info..."
    try {
        $Release     = Invoke-RestMethod -Uri "$GithubApi/releases/latest" -UseBasicParsing
        $Tag         = $Release.tag_name
        $Asset       = $Release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
        if (-not $Asset) {
            Write-Fatal "asset '$AssetName' not found in release $Tag. Check https://github.com/$Repo/releases"
        }
        $DownloadUrl = $Asset.browser_download_url
    } catch {
        Write-Fatal "failed to fetch release info: $_"
    }
}

Write-Info "version : $Tag"
Write-Info "asset   : $AssetName"
Write-Info "install : $InstallDir\$BinaryName"

# Create install directory
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Download binary
$TmpFile = [System.IO.Path]::GetTempFileName() + ".exe"
Write-Info "downloading $DownloadUrl"
try {
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $TmpFile -UseBasicParsing
} catch {
    Write-Fatal "download failed: $_"
}

# Install
$Dest = Join-Path $InstallDir $BinaryName
Move-Item -Force $TmpFile $Dest

# Verify binary runs
try {
    $Version = & $Dest --version 2>&1
    Write-Info "installed: $Version"
} catch {
    Write-Warn "binary installed but could not verify version"
}

# Add to user PATH if not already present
$UserPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [System.Environment]::SetEnvironmentVariable(
        "Path",
        "$UserPath;$InstallDir",
        "User"
    )
    Write-Info "added $InstallDir to your PATH (restart your terminal to apply)"
} else {
    Write-Info "$InstallDir is already in PATH"
}

Write-Host ""
Write-Host "Quick start:" -ForegroundColor Green
Write-Host "  cd your-project"
Write-Host "  knogg init"
Write-Host "  knogg status"
