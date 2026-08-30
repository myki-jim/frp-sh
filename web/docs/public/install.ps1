# frp-sh one-line installer / updater (Windows PowerShell)
#
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
# Env:
#   $env:FRPSH_REPO         override the GitHub fallback repo (default myki-jim/frp-sh)
#   $env:FRPSH_INSTALL_DIR  override the install directory
#   $env:FRPSH_SKIP_INIT='1'  skip the interactive config prompt
$ErrorActionPreference = 'Stop'

# ---- colors ----
$C = @{
  Green  = [char]27 + '[32m'
  Cyan   = [char]27 + '[36m'
  Yellow = [char]27 + '[33m'
  Red    = [char]27 + '[31m'
  Dim    = [char]27 + '[2m'
  Bold   = [char]27 + '[1m'
  Reset  = [char]27 + '[0m'
}
function Say  { Write-Host "$($C.Green)$args$($C.Reset)" }
function Info { Write-Host "$($C.Cyan)$args$($C.Reset)" }
function Warn { Write-Host "$($C.Yellow)$args$($C.Reset)" }
function Fail { Write-Host "$($C.Red)$args$($C.Reset)"; exit 1 }
function Step { Write-Host "`n$($C.Bold)$args$($C.Reset)"; Write-Host "$($C.Dim)----------------------------------------$($C.Reset)" }

# ---- banner ----
Write-Host "$($C.Green)"
Write-Host '  _____ _ ____   ____  _   _  '
Write-Host ' |  ___| |  _ \ / ___|| | | | '
Write-Host ' | |_  | | |_) | \__ \| |_| | '
Write-Host ' |  _| | |  _ <  ___) |  _  | '
Write-Host ' |_|   |_|_| \_\____/|_| |_|  '
Write-Host "$($C.Reset)"

$repo = if ($env:FRPSH_REPO) { $env:FRPSH_REPO } else { 'myki-jim/frp-sh' }
$destDir = if ($env:FRPSH_INSTALL_DIR) { $env:FRPSH_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'frp-sh' }
$exe = Join-Path $destDir 'frp-sh.exe'
$asset = 'frp-sh-windows-x86_64.exe'
$bases = @("https://frp.sh/downloads", "https://github.com/$repo/releases/latest/download")

Write-Host "  $($C.Bold)frp-sh installer$($C.Reset)  (windows/x86_64)"
Write-Host ""

# ---- 1/3 download ----
Step '  [1/3] Downloading frp-sh-windows-x86_64.exe'
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$tmp = Join-Path $destDir 'frp-sh.exe.tmp'
$downloaded = $false
foreach ($b in $bases) {
  try {
    Info "    trying $b/$asset ..."
    Invoke-WebRequest -Uri "$b/$asset" -OutFile $tmp -UseBasicParsing -ErrorAction Stop
    $downloaded = $true
    break
  } catch {
    # try the next source
  }
}
if (-not $downloaded) {
  Fail "error: download failed $asset (tried frp.sh and GitHub Releases; check your network)"
}
Say "    [OK] downloaded $([math]::Round((Get-Item $tmp).Length / 1MB, 1)) MB"
Move-Item -Force $tmp $exe

# ---- 2/3 install ----
Step "  [2/3] Installing to $exe"

# Wintun driver for lan (mesh) mode: put wintun.dll next to the exe
$dll = Join-Path $destDir 'wintun.dll'
if (-not (Test-Path $dll)) {
  try {
    Info '    installing wintun.dll (required for lan mesh mode) ...'
    $wz = Join-Path $destDir 'wintun.zip'
    Invoke-WebRequest -Uri 'https://www.wintun.net/builds/wintun-0.14.1.zip' -OutFile $wz -UseBasicParsing -ErrorAction Stop
    Expand-Archive -Path $wz -DestinationPath (Join-Path $destDir '_wintun_tmp') -Force
    Copy-Item (Join-Path $destDir '_wintun_tmp\wintun\bin\amd64\wintun.dll') $dll -Force
    Remove-Item -Recurse -Force (Join-Path $destDir '_wintun_tmp'), $wz -ErrorAction SilentlyContinue
    Say '    [OK] wintun.dll installed'
  } catch {
    Warn '    [WARN] wintun.dll download failed: lan mesh mode unavailable (game/dev forwarding still work)'
  }
}

$ver = & $exe --version 2>$null | Select-Object -First 1
Say "    [OK] installed: $exe ($ver)"

# add to user PATH (if not already there)
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$destDir*") {
  $newPath = if ([string]::IsNullOrEmpty($userPath)) { $destDir } else { "$userPath;$destDir" }
  [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
  Info "    added $destDir to the user PATH (effective in new terminals)"
}

# ---- 3/3 first-run setup ----
$cfgDir = Join-Path $env:APPDATA 'frp-sh'
$cfg = Join-Path $cfgDir 'config.toml'
$skipInit = $env:FRPSH_SKIP_INIT -eq '1'
if (-not (Test-Path $cfg) -and -not $skipInit) {
  try {
    Step '  [3/3] First-run setup'
    Write-Host '    Which signaling server should frp-sh use?'
    Write-Host "    (the public VPS running 'frp-sh serve'; relay is derived as host:8081)"
    $sig = Read-Host "    $($C.Cyan)Signaling server address$($C.Reset) (e.g. 101.43.41.195:8080) [Enter to skip]"
    if (-not [string]::IsNullOrWhiteSpace($sig)) {
      if ($sig -notmatch '^https?://') { $sig = "http://$sig" }
      $hostOnly = (($sig -replace '^https?://','') -split ':')[0]
      New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
      "signaling_addr = `"$sig`"`nrelay_addr = `"${hostOnly}:8081`"" | Set-Content $cfg -Encoding utf8
      Say "    [OK] config saved: $cfg"
      Say '    ready to go. Run frp-sh to get started.'
    } else {
      Warn "    skipped. Run 'frp-sh config' later to set up your server."
    }
  } catch {
    Warn "    [WARN] interactive setup failed; run 'frp-sh config' manually."
  }
} elseif (-not (Test-Path $cfg)) {
  Info '    (config setup skipped: FRPSH_SKIP_INIT=1)'
}

Write-Host ""
Say "  All done. Run 'frp-sh --help' to get started."
Info '  tip: lan mesh mode needs to run as administrator (creates a virtual NIC).'
