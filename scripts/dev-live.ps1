<#
.SYNOPSIS
    Starts the canonical Tetonic local engine and connects it to the Vite web UI.
.DESCRIPTION
    Runs `cargo run -p tetonic-cli -- ui` with the configured local model, generates a
    secure single-owner connection URL, and opens the live team workspace.
#>
param (
    [string]$Model = "qwen3.5:latest",
    [string]$Database = "../.lokai/ui/workspace.db",
    [int]$Port = 3000,
    [string]$UiOrigin = "http://127.0.0.1:5173",
    [string]$WorkspaceRoot = ""
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
$EngineDir = Join-Path $RepoRoot "engine"
$UiDir = Join-Path $RepoRoot ".lokai\ui"

if (-not (Test-Path $UiDir)) {
    New-Item -ItemType Directory -Path $UiDir -Force | Out-Null
}

$ConnectionFile = Join-Path $UiDir "connection.url"
if (Test-Path $ConnectionFile) {
    Remove-Item -Force $ConnectionFile
}

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "  TETONIC ENGINE - LIVE LOCAL DAEMON LAUNCHER            " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "Model:          $Model" -ForegroundColor Gray
Write-Host "Database:       $Database" -ForegroundColor Gray
Write-Host "UI Origin:      $UiOrigin" -ForegroundColor Gray
Write-Host "Workspace Root: $WorkspaceRoot" -ForegroundColor Gray
Write-Host ""

# Check Ollama health
try {
    $resp = Invoke-RestMethod -Uri "http://127.0.0.1:11434/api/tags" -Method Get -TimeoutSec 3 -ErrorAction SilentlyContinue
    if ($resp -and $resp.models) {
        $installed = $resp.models | ForEach-Object { $_.name }
        if ($installed -contains $Model) {
            Write-Host "[✓] Ollama online with model '$Model' ready." -ForegroundColor Green
        } else {
            Write-Host "[!] Ollama is online, but '$Model' was not found in installed models:" -ForegroundColor Yellow
            $installed | ForEach-Object { Write-Host "    - $_" -ForegroundColor DarkGray }
        }
    }
} catch {
    Write-Host "[!] Warning: Could not contact Ollama on http://127.0.0.1:11434. Local inference may fail if Ollama is not started." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Starting Tetonic Local Engine daemon on port $Port..." -ForegroundColor Green

$cargoArgs = @(
    "run", "-p", "tetonic-cli", "--", "ui",
    "--database", $Database,
    "--model", $Model,
    "--port", $Port,
    "--ui-origin", $UiOrigin,
    "--connection-file", $ConnectionFile
)

if (-not [string]::IsNullOrWhiteSpace($WorkspaceRoot)) {
    $cargoArgs += @("--workspace-root", $WorkspaceRoot)
}

# Start-Process joins ArgumentList without preserving boundaries; quote paths
# (including the connection file under user profiles containing spaces).
$quotedCargoArgs = $cargoArgs | ForEach-Object {
    if ($_ -match '["\r\n]') { throw 'Launcher arguments cannot contain quotes or newlines.' }
    '"' + ($_ -replace '(\\+)$', '$1$1') + '"'
}
$process = Start-Process -FilePath "cargo" -ArgumentList $quotedCargoArgs -WorkingDirectory $EngineDir -WindowStyle Hidden -PassThru

# Wait for connection.url to be written
$waitedSec = 0
while (-not (Test-Path $ConnectionFile) -and $waitedSec -lt 40) {
    Start-Sleep -Seconds 1
    $waitedSec++
    if ($process.HasExited) {
        Write-Host "[X] Engine failed to start (exit code $($process.ExitCode))." -ForegroundColor Red
        exit 1
    }
}

if (Test-Path $ConnectionFile) {
    $connectUrl = (Get-Content $ConnectionFile -Raw).Trim()
    Write-Host ""
    Write-Host "==========================================================" -ForegroundColor Green
    Write-Host "  TETONIC ENGINE ONLINE (Port $Port)                     " -ForegroundColor Green
    Write-Host "==========================================================" -ForegroundColor Green
    Write-Host "Connection URL:" -ForegroundColor White
    Write-Host "  $connectUrl" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Opening web UI..." -ForegroundColor Gray
    Start-Process $connectUrl
} else {
    Write-Host "[!] Timed out waiting for connection URL file." -ForegroundColor Red
}

# Keep script open while daemon runs
try {
    $process.WaitForExit()
} catch {
    if (-not $process.HasExited) {
        $process.Kill()
    }
}
