# frp-sh 0.4 installer. Elevation is confined to this installation process.
param(
    [switch]$Elevated,
    [string]$OwnerSid = '',
    [ValidateSet('auto','zh-CN','en')][string]$Lang = 'auto'
)
$ErrorActionPreference = 'Stop'
if ($Lang -eq 'auto') { $Lang = if ((Get-Culture).Name -like 'zh*') {'zh-CN'} else {'en'} }
function Message([string]$English,[string]$Chinese) { if ($Lang -eq 'zh-CN') { Write-Host $Chinese } else { Write-Host $English } }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Message 'Installing the network service requires one administrator approval.' '安装网络辅助服务需要一次管理员授权。'
    $bootstrap = Join-Path ([IO.Path]::GetTempPath()) ('frpsh-install-' + [guid]::NewGuid().ToString('N') + '.ps1')
    try {
        if ($PSCommandPath) { Copy-Item -LiteralPath $PSCommandPath -Destination $bootstrap }
        else { Invoke-WebRequest -Uri 'https://frp.sh/install.ps1' -OutFile $bootstrap -UseBasicParsing }
        $arguments = @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"' + $bootstrap + '"'),'-Elevated','-OwnerSid',$identity.User.Value,'-Lang',$Lang)
        $child = Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru
        if ($child.ExitCode -ne 0) { throw 'Installation failed. Run the installer from an administrator terminal to see the detailed error.' }
    } finally { Remove-Item -LiteralPath $bootstrap -ErrorAction SilentlyContinue }
    $env:Path += ';' + (Join-Path $env:ProgramFiles 'frp-sh')
    Message 'Installed. Run frp-sh from your normal terminal; LAN sessions no longer request UAC.' '安装完成。请在普通终端运行 frp-sh，LAN 会话不再请求 UAC。'
    return
}
if (-not $OwnerSid) { $OwnerSid = $identity.User.Value }
$null = New-Object Security.Principal.SecurityIdentifier($OwnerSid)
$destination = Join-Path $env:ProgramFiles 'frp-sh'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
& icacls.exe $destination /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot protect the installation directory' }
$stage = Join-Path $destination ('staging-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
$base = 'https://github.com/myki-jim/frp-sh/releases/latest/download'
function Download-Verified([string]$Asset,[string]$Output) {
    Invoke-WebRequest -Uri ($base + '/' + $Asset) -OutFile $Output -UseBasicParsing
    $checksum = ((Invoke-WebRequest -Uri ($base + '/' + $Asset + '.sha256') -UseBasicParsing).Content.Trim() -split '\s+')[0]
    if ($checksum -notmatch '^[a-fA-F0-9]{64}$' -or (Get-FileHash -Algorithm SHA256 -LiteralPath $Output).Hash -ne $checksum) { throw "Checksum mismatch: $Asset" }
}
try {
    Message 'Downloading and verifying client and network helper...' '正在下载并校验客户端和网络辅助程序…'
    Download-Verified 'frp-sh-windows-x86_64.exe' (Join-Path $stage 'frp-sh.exe')
    Download-Verified 'frp-sh-net-windows-x86_64.exe' (Join-Path $stage 'frp-sh-net.exe')
    $archive = Join-Path $stage 'wintun.zip'
    Invoke-WebRequest -Uri 'https://www.wintun.net/builds/wintun-0.14.1.zip' -OutFile $archive -UseBasicParsing
    Expand-Archive -LiteralPath $archive -DestinationPath $stage
    $dll = Join-Path $stage 'wintun/bin/amd64/wintun.dll'
    if ((Get-AuthenticodeSignature -LiteralPath $dll).Status -ne 'Valid') { throw 'Wintun driver signature verification failed' }
    $existing = Get-Service -Name FrpShNetwork -ErrorAction SilentlyContinue
    if ($existing) { Stop-Service FrpShNetwork -ErrorAction Stop }
    # Keep one previous executable pair for installation rollback.
    foreach ($name in @('frp-sh.exe','frp-sh-net.exe','wintun.dll','helper.toml')) {
        $current = Join-Path $destination $name
        if (Test-Path -LiteralPath $current) { Copy-Item -LiteralPath $current -Destination ($current + '.previous') -Force }
    }
    try {
        Copy-Item -LiteralPath (Join-Path $stage 'frp-sh.exe') -Destination (Join-Path $destination 'frp-sh.exe') -Force
        Copy-Item -LiteralPath (Join-Path $stage 'frp-sh-net.exe') -Destination (Join-Path $destination 'frp-sh-net.exe') -Force
        Copy-Item -LiteralPath $dll -Destination (Join-Path $destination 'wintun.dll') -Force
        [IO.File]::WriteAllText((Join-Path $destination 'helper.toml'),('allowed_sid = "' + $OwnerSid + '"'),(New-Object Text.UTF8Encoding($false)))
        if (-not $existing) {
            & sc.exe create FrpShNetwork binPath= ('"' + (Join-Path $destination 'frp-sh-net.exe') + '"') start= auto DisplayName= 'frp-sh Network Helper' | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'Service registration failed' }
        }
        & sc.exe failure FrpShNetwork reset= 86400 actions= restart/3000/restart/10000/restart/30000 | Out-Null
        Start-Service FrpShNetwork
        Start-Sleep -Milliseconds 500
        if ((Get-Service FrpShNetwork).Status -ne 'Running') { throw 'Network helper did not start' }
    } catch {
        Stop-Service FrpShNetwork -ErrorAction SilentlyContinue
        if (-not $existing) { & sc.exe delete FrpShNetwork | Out-Null }
        foreach ($name in @('frp-sh.exe','frp-sh-net.exe','wintun.dll')) {
            $current = Join-Path $destination $name
            if (Test-Path -LiteralPath ($current + '.previous')) { Copy-Item -LiteralPath ($current + '.previous') -Destination $current -Force }
        }
        if ($existing) { Start-Service FrpShNetwork -ErrorAction SilentlyContinue }
        throw
    }
    $machinePath = [Environment]::GetEnvironmentVariable('Path','Machine')
    if (($machinePath -split ';') -notcontains $destination) { [Environment]::SetEnvironmentVariable('Path',($machinePath.TrimEnd(';') + ';' + $destination),'Machine') }
    Message 'Installation complete. Reopen a normal terminal and run frp-sh.' '安装完成。重新打开普通终端后运行 frp-sh。'
} finally {
    $resolved = [IO.Path]::GetFullPath($stage)
    if (-not $resolved.StartsWith(([IO.Path]::GetFullPath($destination) + '\'),[StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe staging path' }
    Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
}
