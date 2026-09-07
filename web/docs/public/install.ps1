# frp-sh 0.4 installer. Elevation is confined to this installation process.
param(
    [switch]$Elevated,
    [string]$OwnerSid = '',
    [ValidateSet('auto','zh-CN','en')][string]$Lang = 'auto'
)
$ErrorActionPreference = 'Stop'
if ($Lang -eq 'auto') { $Lang = if ((Get-Culture).Name -like 'zh*') {'zh-CN'} else {'en'} }
# Unicode escapes keep both irm | iex and Windows PowerShell -File encoding-safe.
function Message([string]$English,[string]$Chinese) { if ($Lang -eq 'zh-CN') { Write-Host ([Text.RegularExpressions.Regex]::Unescape($Chinese)) } else { Write-Host $English } }
function Get-FrpShPath([string]$CurrentPath,[string]$Destination) {
    $entries = @($CurrentPath -split ';' | Where-Object {
        $_.Trim() -and $_.Trim().Trim('"').TrimEnd('\') -ine $Destination.TrimEnd('\')
    })
    return (@($Destination) + $entries) -join ';'
}
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Message 'Installing the network service requires one administrator approval.' '\u5b89\u88c5\u7f51\u7edc\u8f85\u52a9\u670d\u52a1\u9700\u8981\u4e00\u6b21\u7ba1\u7406\u5458\u6388\u6743\u3002'
    $bootstrap = Join-Path ([IO.Path]::GetTempPath()) ('frpsh-install-' + [guid]::NewGuid().ToString('N') + '.ps1')
    try {
        if ($PSCommandPath) { Copy-Item -LiteralPath $PSCommandPath -Destination $bootstrap }
        else { Invoke-WebRequest -Uri 'https://frp.sh/install.ps1' -OutFile $bootstrap -UseBasicParsing -TimeoutSec 30 }
        $arguments = @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"' + $bootstrap + '"'),'-Elevated','-OwnerSid',$identity.User.Value,'-Lang',$Lang)
        $child = Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru
        if ($child.ExitCode -ne 0) { throw 'Installation failed. Run the installer from an administrator terminal to see the detailed error.' }
    } finally { Remove-Item -LiteralPath $bootstrap -ErrorAction SilentlyContinue }
    $env:Path = Get-FrpShPath $env:Path (Join-Path $env:ProgramFiles 'frp-sh')
    & (Join-Path $env:ProgramFiles 'frp-sh/frp-sh.exe') --version
    Message 'Installed. Run frp-sh from your normal terminal; LAN sessions no longer request UAC.' '\u5b89\u88c5\u5b8c\u6210\u3002\u8bf7\u5728\u666e\u901a\u7ec8\u7aef\u8fd0\u884c frp-sh\uff0cLAN \u4f1a\u8bdd\u4e0d\u518d\u8bf7\u6c42 UAC\u3002'
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
$previousProgress = $ProgressPreference
$ProgressPreference = 'SilentlyContinue' # Avoid PowerShell 5.1's per-chunk progress overhead.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
try {
$checksums = @{}
$manifestFile = Join-Path $stage 'release-manifest.txt'
try {
    Invoke-WebRequest -Uri 'https://frp.sh/release-manifest.txt' -OutFile $manifestFile -UseBasicParsing -TimeoutSec 10
} catch { Remove-Item -LiteralPath $manifestFile -ErrorAction SilentlyContinue }
if (Test-Path -LiteralPath $manifestFile) {
    $lines = [IO.File]::ReadAllLines($manifestFile)
    $tag = 'v' + $lines[0].Trim()
    foreach ($line in $lines | Select-Object -Skip 1) {
        if ($line -match '^([a-fA-F0-9]{64})  (frp-sh-[A-Za-z0-9.-]+)$') {
            if ($checksums.ContainsKey($Matches[2])) { throw 'Duplicate manifest entry' }
            $checksums[$Matches[2]] = $Matches[1]
        } else { throw 'Invalid release manifest' }
    }
} else {
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/myki-jim/frp-sh/releases/latest' -TimeoutSec 15
    $tag = $release.tag_name
}
if ($tag -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?$') { throw 'Invalid release tag' }
$base = 'https://github.com/myki-jim/frp-sh/releases/download/' + $tag
function Download-Verified([string]$Asset,[string]$Output) {
    $checksum = $checksums[$Asset]
    if (-not $checksum) {
        if (Test-Path -LiteralPath $manifestFile) { throw "Missing manifest entry: $Asset" }
        $checksumFile = $Output + '.sha256'
        Invoke-WebRequest -Uri ($base + '/' + $Asset + '.sha256') -OutFile $checksumFile -UseBasicParsing -TimeoutSec 15
        $checksum = ([IO.File]::ReadAllText($checksumFile, [Text.Encoding]::UTF8).Trim() -split '\s+')[0]
    }
    if ($checksum -notmatch '^[a-fA-F0-9]{64}$') { throw "Checksum mismatch: $Asset" }
    foreach ($source in @('https://frp.sh/downloads', $base)) {
        $downloadTimeout = if ($source -eq $base) { 60 } else { 15 }
        for ($attempt = 0; $attempt -lt 2; $attempt++) {
            try {
                Invoke-WebRequest -Uri ($source + '/' + $Asset) -OutFile $Output -UseBasicParsing -TimeoutSec $downloadTimeout
                if ((Get-FileHash -Algorithm SHA256 -LiteralPath $Output).Hash -eq $checksum) { return }
                break # Corrupt/stale mirror: switch sources instead of downloading it again.
            } catch {
                if ($attempt -eq 0) { Start-Sleep -Milliseconds 300 }
            }
        }
        Remove-Item -LiteralPath $Output -ErrorAction SilentlyContinue
        Message 'Switching download source...' '\u6b63\u5728\u5207\u6362\u4e0b\u8f7d\u6e90\u2026'
    }
    throw "Checksum mismatch: $Asset (all download sources failed)"
}

    Message 'Downloading and verifying client and network helper...' '\u6b63\u5728\u4e0b\u8f7d\u5e76\u6821\u9a8c\u5ba2\u6237\u7aef\u548c\u7f51\u7edc\u8f85\u52a9\u7a0b\u5e8f\u2026'
    Download-Verified 'frp-sh-client-windows-x86_64.exe' (Join-Path $stage 'frp-sh.exe')
    Download-Verified 'frp-sh-net-windows-x86_64.exe' (Join-Path $stage 'frp-sh-net.exe')
    $installedDll = Join-Path $destination 'wintun.dll'
    $dll = Join-Path $stage 'wintun.dll'
    if ((Test-Path -LiteralPath $installedDll) -and (Get-AuthenticodeSignature -LiteralPath $installedDll).Status -eq 'Valid') {
        Copy-Item -LiteralPath $installedDll -Destination $dll
    } else {
        $archive = Join-Path $stage 'wintun.zip'
        Invoke-WebRequest -Uri 'https://www.wintun.net/builds/wintun-0.14.1.zip' -OutFile $archive -UseBasicParsing -TimeoutSec 60
        Expand-Archive -LiteralPath $archive -DestinationPath $stage
        $dll = Join-Path $stage 'wintun/bin/amd64/wintun.dll'
    }
    if ((Get-AuthenticodeSignature -LiteralPath $dll).Status -ne 'Valid') { throw 'Wintun driver signature verification failed' }
    $existing = Get-Service -Name FrpShNetwork -ErrorAction SilentlyContinue
    $previousFiles = @{}
    # Keep the previous executable pair and policy for installation rollback.
    foreach ($name in @('frp-sh.exe','frp-sh-net.exe','wintun.dll','helper.toml')) {
        $current = Join-Path $destination $name
        if (Test-Path -LiteralPath $current) { Copy-Item -LiteralPath $current -Destination ($current + '.previous') -Force; $previousFiles[$name] = $true }
    }
    try {
        if ($existing) { Stop-Service FrpShNetwork -ErrorAction Stop }
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
        foreach ($name in @('frp-sh.exe','frp-sh-net.exe','wintun.dll','helper.toml')) {
            $current = Join-Path $destination $name
            if ($previousFiles.ContainsKey($name)) { Copy-Item -LiteralPath ($current + '.previous') -Destination $current -Force }
            else { Remove-Item -LiteralPath $current -ErrorAction SilentlyContinue }
        }
        if ($existing) { Start-Service FrpShNetwork -ErrorAction SilentlyContinue }
        throw
    }
    $machinePath = [Environment]::GetEnvironmentVariable('Path','Machine')
    $newMachinePath = Get-FrpShPath $machinePath $destination
    if ($newMachinePath -ne $machinePath) { [Environment]::SetEnvironmentVariable('Path',$newMachinePath,'Machine') }
    # irm | iex executes here when the caller is already elevated. Its process
    # PATH otherwise keeps resolving a pre-0.4 LocalAppData installation.
    $env:Path = Get-FrpShPath $env:Path $destination
    & (Join-Path $destination 'frp-sh.exe') --version
    Message 'Installation complete. frp-sh is ready in this terminal.' '\u5b89\u88c5\u5b8c\u6210\u3002\u5f53\u524d\u7ec8\u7aef\u53ef\u76f4\u63a5\u8fd0\u884c frp-sh\u3002'
} finally {
    $ProgressPreference = $previousProgress
    $resolved = [IO.Path]::GetFullPath($stage)
    if (-not $resolved.StartsWith(([IO.Path]::GetFullPath($destination) + '\'),[StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe staging path' }
    Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
}
