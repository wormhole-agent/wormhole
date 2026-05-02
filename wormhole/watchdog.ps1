# watchdog.ps1
#
# Keep wormhole.exe alive. If it dies, restart it. Logs to ~/wormhole/logs/.
# Registered by wormhole-task.xml as a per-user scheduled task that fires at logon.

$ErrorActionPreference = 'Continue'
$home_dir = Join-Path $env:USERPROFILE 'wormhole'
$log_dir  = Join-Path $home_dir 'logs'
if (-not (Test-Path $log_dir)) { New-Item -ItemType Directory -Path $log_dir | Out-Null }
$log = Join-Path $log_dir 'watchdog.log'

function Write-Log($msg) {
  $stamp = Get-Date -Format 'yyyy-MM-ddTHH:mm:ssK'
  "$stamp $msg" | Add-Content -Path $log -Encoding utf8
}

# Find the binary. Prefer the per-user install, then PATH.
$bin = Join-Path $env:LOCALAPPDATA 'Programs\WormHole\bin\wormhole.exe'
if (-not (Test-Path $bin)) {
  $bin = (Get-Command wormhole.exe -ErrorAction SilentlyContinue).Source
}
if (-not $bin) {
  Write-Log "FATAL: wormhole.exe not found in PATH. Aborting watchdog."
  exit 1
}

Write-Log "Watchdog starting; bin=$bin"

while ($true) {
  $proc = Get-Process wormhole -ErrorAction SilentlyContinue
  if (-not $proc) {
    Write-Log "wormhole.exe not running; starting..."
    Start-Process -FilePath $bin -ArgumentList 'serve' -WindowStyle Hidden
  }
  Start-Sleep -Seconds 30
}
