# frp-sh 一键安装 / 更新脚本（Windows PowerShell）
#
# 用法（在 PowerShell 中执行）:
#   irm https://frp.sh/install.ps1 | iex
#
# 说明:
#   - 自动下载 GitHub Releases 最新版 Windows 二进制
#   - 安装到 %LOCALAPPDATA%\frp-sh 并加入用户 PATH
#   - 重复执行即为更新（覆盖安装）
#
# 可选环境变量:
#   $env:FRPSH_REPO       覆盖下载仓库（默认 frp-sh/frp-sh）
#   $env:FRPSH_INSTALL_DIR 覆盖安装目录
$ErrorActionPreference = 'Stop'

$repo = if ($env:FRPSH_REPO) { $env:FRPSH_REPO } else { 'myki-jim/frp-sh' }
$base = "https://github.com/$repo/releases/latest/download"
$destDir = if ($env:FRPSH_INSTALL_DIR) { $env:FRPSH_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'frp-sh' }
$exe = Join-Path $destDir 'frp-sh.exe'
$asset = 'frp-sh-windows-x86_64.exe'
$url = "$base/$asset"

Write-Host "==> frp-sh 安装器 (windows/x86_64)"
Write-Host "    下载: $url"
Write-Host "    安装到: $exe"

New-Item -ItemType Directory -Force -Path $destDir | Out-Null
$tmp = Join-Path $destDir 'frp-sh.exe.tmp'
Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
Move-Item -Force $tmp $exe

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
