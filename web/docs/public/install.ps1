# frp-sh one-line installer / updater (Windows PowerShell)
#
# Usage (run in PowerShell):
#   irm https://frp.sh/install.ps1 | iex
#
# Notes:
#   - downloads the latest Windows binary (frp.sh official source first,
#     GitHub Releases as fallback)
#   - installs to %LOCALAPPDATA%\frp-sh and adds it to the user PATH
#   - re-running updates in place (overwrites)
#   - on first install (no config yet) it interactively asks for your
#     signaling server and writes the config; press Enter to skip
#
# Optional environment variables:
#   $env:FRPSH_REPO         override the GitHub fallback repo (default myki-jim/frp-sh)
#   $env:FRPSH_INSTALL_DIR  override the install directory
#   $env:FRPSH_SKIP_INIT    set to '1' to skip the interactive config prompt
$ErrorActionPreference = 'Stop'

$repo = if ($env:FRPSH_REPO) { $env:FRPSH_REPO } else { 'myki-jim/frp-sh' }
$destDir = if ($env:FRPSH_INSTALL_DIR) { $env:FRPSH_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'frp-sh' }
$exe = Join-Path $destDir 'frp-sh.exe'
$asset = 'frp-sh-windows-x86_64.exe'

# Download sources: Cloudflare official source first, GitHub Releases fallback
$bases = @("https://frp.sh/downloads", "https://github.com/$repo/releases/latest/download")

Write-Host "==> frp-sh installer (windows/x86_64)"
Write-Host "    installing to: $exe"

New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$tmp = Join-Path $destDir 'frp-sh.exe.tmp'
$downloaded = $false
foreach ($b in $bases) {
  try {
    Write-Host "    trying: $b/$asset"
    Invoke-WebRequest -Uri "$b/$asset" -OutFile $tmp -UseBasicParsing -ErrorAction Stop
    $downloaded = $true
    break
  } catch {
    # try the next source
  }
}
if (-not $downloaded) {
  throw "download failed $asset (tried frp.sh and GitHub Releases; check your network)"
}
Move-Item -Force $tmp $exe

# Wintun driver for lan (mesh) mode: put wintun.dll next to the exe
$dll = Join-Path $destDir 'wintun.dll'
if (-not (Test-Path $dll)) {
  try {
    Write-Host "    downloading wintun.dll (required for lan mesh mode) ..."
    $wz = Join-Path $destDir 'wintun.zip'
    Invoke-WebRequest -Uri 'https://www.wintun.net/builds/wintun-0.14.1.zip' -OutFile $wz -UseBasicParsing -ErrorAction Stop
    Expand-Archive -Path $wz -DestinationPath (Join-Path $destDir '_wintun_tmp') -Force
    Copy-Item (Join-Path $destDir '_wintun_tmp\wintun\bin\amd64\wintun.dll') $dll -Force
    Remove-Item -Recurse -Force (Join-Path $destDir '_wintun_tmp'), $wz -ErrorAction SilentlyContinue
    Write-Host "    wintun.dll installed"
  } catch {
    Write-Host "    [warn] wintun.dll download failed: lan mesh mode will be unavailable (game/dev forwarding still work)"
  }
}

# Add to user PATH (if not already there)
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$destDir*") {
  $newPath = if ([string]::IsNullOrEmpty($userPath)) { $destDir } else { "$userPath;$destDir" }
  [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
  Write-Host "==> added $destDir to the user PATH (effective in new terminals)"
}

$ver = & $exe --version 2>$null | Select-Object -First 1
Write-Host "==> installed: $exe ($ver)"

# First-run initialization: ask for the signaling server and write the config
$cfgDir = Join-Path $env:APPDATA 'frp-sh'
$cfg = Join-Path $cfgDir 'config.toml'
$skipInit = $env:FRPSH_SKIP_INIT -eq '1'
if (-not (Test-Path $cfg) -and -not $skipInit) {
  try {
    Write-Host ""
    Write-Host "==> First run setup: which signaling server should frp-sh use?"
    Write-Host "    (the public VPS running 'frp-sh serve'; relay is derived as host:8081)"
    $sig = Read-Host "    Signaling server address (e.g. 101.43.41.195:8080) [Enter to skip]"
    if (-not [string]::IsNullOrWhiteSpace($sig)) {
      if ($sig -notmatch '^https?://') { $sig = "http://$sig" }
      $hostOnly = (($sig -replace '^https?://','') -split ':')[0]
      New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
      "signaling_addr = `"$sig`"`nrelay_addr = `"${hostOnly}:8081`"" | Set-Content $cfg -Encoding utf8
      Write-Host "    config saved: $cfg"
      Write-Host "    ready to go. Run 'frp-sh' to get started."
    } else {
      Write-Host "    skipped. Run 'frp-sh config' later to set up your server."
    }
  } catch {
    Write-Host "    [warn] interactive setup failed; run 'frp-sh config' manually."
  }
} elseif (-not (Test-Path $cfg)) {
  Write-Host "    (config setup skipped: FRPSH_SKIP_INIT=1)"
}

Write-Host "    run 'frp-sh --help' to get started; re-run this command to update."
Write-Host "    note: lan mesh mode needs to run as administrator (creates a virtual NIC)."
