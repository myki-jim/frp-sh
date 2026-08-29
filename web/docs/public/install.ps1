# frp-sh 一键安装 / 更新脚本（Windows PowerShell）
#
# 用法（在 PowerShell 中执行）:
#   irm https://frp.sh/install.ps1 | iex
#
# 说明:
#   - 自动下载最新版 Windows 二进制（frp.sh 官方源优先，GitHub Releases 兜底）
#   - 安装到 %LOCALAPPDATA%\frp-sh 并加入用户 PATH
#   - 重复执行即为更新（覆盖安装）
#
# 可选环境变量:
#   $env:FRPSH_REPO       覆盖 GitHub 兜底仓库（默认 myki-jim/frp-sh）
#   $env:FRPSH_INSTALL_DIR 覆盖安装目录
$ErrorActionPreference = 'Stop'

$repo = if ($env:FRPSH_REPO) { $env:FRPSH_REPO } else { 'myki-jim/frp-sh' }
$destDir = if ($env:FRPSH_INSTALL_DIR) { $env:FRPSH_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'frp-sh' }
$exe = Join-Path $destDir 'frp-sh.exe'
$asset = 'frp-sh-windows-x86_64.exe'

# 下载源：Cloudflare 官方源优先（国内可达），GitHub Releases 兜底
$bases = @("https://frp.sh/downloads", "https://github.com/$repo/releases/latest/download")

Write-Host "==> frp-sh 安装器 (windows/x86_64)"
Write-Host "    安装到: $exe"

New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$tmp = Join-Path $destDir 'frp-sh.exe.tmp'
$downloaded = $false
foreach ($b in $bases) {
  try {
    Write-Host "    尝试: $b/$asset"
    Invoke-WebRequest -Uri "$b/$asset" -OutFile $tmp -UseBasicParsing -ErrorAction Stop
    $downloaded = $true
    break
  } catch {
    # 继续尝试下一个源
  }
}
if (-not $downloaded) {
  throw "下载失败 $asset（已尝试 frp.sh 与 GitHub Releases，请检查网络）"
}
Move-Item -Force $tmp $exe

# Windows 组网（lan）需要 Wintun 驱动库：下载 wintun.dll 放到 exe 旁边
$dll = Join-Path $destDir 'wintun.dll'
if (-not (Test-Path $dll)) {
  try {
    Write-Host "    下载 wintun.dll（lan 组网模式需要）..."
    $wz = Join-Path $destDir 'wintun.zip'
    Invoke-WebRequest -Uri 'https://www.wintun.net/builds/wintun-0.14.1.zip' -OutFile $wz -UseBasicParsing -ErrorAction Stop
    Expand-Archive -Path $wz -DestinationPath (Join-Path $destDir '_wintun_tmp') -Force
    Copy-Item (Join-Path $destDir '_wintun_tmp\wintun\bin\amd64\wintun.dll') $dll -Force
    Remove-Item -Recurse -Force (Join-Path $destDir '_wintun_tmp'), $wz -ErrorAction SilentlyContinue
    Write-Host "    wintun.dll 已安装"
  } catch {
    Write-Host "    [警告] wintun.dll 下载失败：lan 组网模式将不可用（game/dev 转发模式不受影响）"
  }
}

# 加入用户 PATH（若尚未包含）
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$destDir*") {
  $newPath = if ([string]::IsNullOrEmpty($userPath)) { $destDir } else { "$userPath;$destDir" }
  [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
  Write-Host "==> 已将 $destDir 加入用户 PATH（新开的终端生效）"
}

$ver = & $exe --version 2>$null | Select-Object -First 1
Write-Host "==> 安装完成: $exe ($ver)"
Write-Host "    运行 frp-sh --help 开始使用；重新执行本命令即可更新。"
Write-Host "    提示: lan 组网模式需以管理员身份运行（创建虚拟网卡）。"
