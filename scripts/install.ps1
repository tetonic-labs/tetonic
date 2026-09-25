# Tetonic Installer for Windows PowerShell
# Usage:
#   irm https://raw.githubusercontent.com/tetonic-labs/tetonic/main/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"

$repo = if ($env:TETONIC_REPO) { $env:TETONIC_REPO } else { "tetonic-labs/tetonic" }
$asset = "tetonic-windows-x64.zip"
$installDir = Join-Path $HOME ".tetonic\bin"

Write-Host "==> Installing Tetonic for Windows from $repo..." -ForegroundColor Cyan

$releaseBaseUrl = "https://github.com/$repo/releases/latest/download"
$downloadUrl = "$releaseBaseUrl/$asset"
$checksumUrl = "$releaseBaseUrl/checksums.txt"

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("tetonic-install-" + [System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

try {
    $zipPath = Join-Path $tempDir $asset
    $checksumPath = Join-Path $tempDir "checksums.txt"

    Write-Host "==> Downloading $asset..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath -UseBasicParsing

    Write-Host "==> Downloading checksums..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath -UseBasicParsing

    Write-Host "==> Verifying SHA256 checksum..." -ForegroundColor Cyan
    $hash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLower()

    $checksumLine = Get-Content $checksumPath | Where-Object { $_ -match $asset }
    if ($checksumLine) {
        $expectedHash = ($checksumLine -split "\s+")[0].ToLower()
        if ($hash -ne $expectedHash) {
            throw "Checksum verification failed! Expected: $expectedHash, Actual: $hash"
        }
        Write-Host "Checksum verified: $hash" -ForegroundColor Green
    } else {
        Write-Host "Warning: $asset not found in checksums.txt. Proceeding." -ForegroundColor Yellow
    }

    Write-Host "==> Extracting binaries..." -ForegroundColor Cyan
    $extractDir = Join-Path $tempDir "extracted"
    Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

    if (-not (Test-Path $installDir)) {
        New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    }

    Copy-Item (Join-Path $extractDir "tetonic.exe") (Join-Path $installDir "tetonic.exe") -Force
    Copy-Item (Join-Path $extractDir "tetonicd.exe") (Join-Path $installDir "tetonicd.exe") -Force

    # Ensure $installDir is in user PATH
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$installDir*") {
        Write-Host "==> Adding $installDir to User PATH..." -ForegroundColor Cyan
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$installDir", "User")
        $env:Path = "$env:Path;$installDir"
    }

    Write-Host ""
    Write-Host "==========================================================" -ForegroundColor Green
    Write-Host "  Tetonic installed successfully to $installDir\tetonic.exe" -ForegroundColor Green
    Write-Host "==========================================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Open a new terminal window and run 'tetonic --help' to get started." -ForegroundColor Cyan

} finally {
    Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
}
