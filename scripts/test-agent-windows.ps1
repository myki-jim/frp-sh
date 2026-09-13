# Disposable GitHub runner only: exercise real SCM virtual accounts and owner ACLs.
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true') { throw 'Run this isolated system-service test in CI only' }
$destination = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'frp-sh'
$data = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'frp-sh'
if ((Test-Path -LiteralPath $destination) -or (Test-Path -LiteralPath $data)) { throw 'Test requires an empty installation' }
foreach ($name in @('FrpShClient','FrpShServer','FrpShNetwork')) {
    if (Get-Service $name -ErrorAction SilentlyContinue) { throw 'Test service already exists' }
}
$user = 'frpshsvc' + [guid]::NewGuid().ToString('N').Substring(0,8)
$password = ConvertTo-SecureString ([guid]::NewGuid().ToString('N') + 'aA!9') -AsPlainText -Force
$account = New-LocalUser -Name $user -Password $password -AccountNeverExpires
$credential = [Management.Automation.PSCredential]::new("$env:COMPUTERNAME\$user",$password)
$results = Join-Path $env:RUNNER_TEMP ('frpsh-agent-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $destination,$results | Out-Null
$exe = Join-Path $destination 'frp-sh.exe'
function Run-Owner([string]$Arguments,[string]$Name) {
    $out = Join-Path $results ($Name + '.out')
    $err = Join-Path $results ($Name + '.err')
    $child = Start-Process -FilePath $exe -ArgumentList $Arguments -Credential $credential -LoadUserProfile -WorkingDirectory $destination -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    if ($child.ExitCode -ne 0) { Get-Content $err; throw ('Ordinary owner command failed: ' + $Name) }
    return [IO.File]::ReadAllText($out)
}
function Install-TestService([bool]$Server) {
    $role = if ($Server) { 'server' } else { 'client' }
    $configPath = Join-Path (Join-Path $data $role) 'config.toml'
    $enabled = if ($Server) { 'true' } else { 'false' }
    $profile = if ($Server) { '' } else { 'friends' }
    $config = 'uuid = "' + [guid]::NewGuid().ToString() + '"' + "`n"
    if ($Server) { $config += "[server]`naddr = '127.0.0.1:0'`nrelay_addr = '127.0.0.1:0'`nudp_addr = '127.0.0.1:0'`n" }
    else { $config += "[profiles.friends]`nname = 'friends'`nserver = 'http://127.0.0.1:1'`nroom = '1234'`nmode = 'lan'`n" }
    $job = "schema_version = 1`nenabled = $enabled`nserver = " + $Server.ToString().ToLowerInvariant() + "`nprofile = '$profile'`nconfig = '$configPath'`nowner_sid = '" + $account.SID.Value + "'`n"
    $snapshot = @{executable=$exe;owner_sid=$account.SID.Value;server=$Server;config=$config;job=$job} | ConvertTo-Json -Compress
    $path = Join-Path $results ($role + '.json')
    [IO.File]::WriteAllText($path,$snapshot,[Text.UTF8Encoding]::new($false))
    $digest = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    & $exe --plain agent install-elevated --snapshot $path --digest $digest
    if ($LASTEXITCODE -ne 0) { throw ('Native service installation failed: ' + $role) }
}
try {
    & icacls.exe $destination /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Binary ACL failed' }
    Copy-Item target/debug/frp-sh.exe,target/debug/frp-sh-net.exe -Destination $destination
    & icacls.exe $results /grant ('*' + $account.SID.Value + ':(OI)(CI)M') | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Result ACL failed' }
    [IO.File]::WriteAllText((Join-Path $destination 'helper.toml'),('allowed_sid = "' + $account.SID.Value + '"'),[Text.UTF8Encoding]::new($false))
    & sc.exe create FrpShNetwork binPath= ('"' + (Join-Path $destination 'frp-sh-net.exe') + '"') start= demand | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Helper service creation failed' }
    Start-Service FrpShNetwork
    Install-TestService $true
    Install-TestService $false
    $until = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 500
        $out = Join-Path $results 'status.out'
        $child = Start-Process -FilePath $exe -ArgumentList '--json status' -Credential $credential -LoadUserProfile -WorkingDirectory $destination -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput $out
        $status = [IO.File]::ReadAllText($out) | ConvertFrom-Json
        $server = $status.processes | Where-Object role -eq 'server_agent'
        $client = $status.processes | Where-Object role -eq 'agent'
        if ($server.lifecycle.phase -eq 'serving' -and $client.lifecycle.phase -eq 'stopped') { break }
    } while ([DateTime]::UtcNow -lt $until)
    if ($server.lifecycle.phase -ne 'serving' -or $client.lifecycle.phase -ne 'stopped') { throw 'Cross-account service status did not become ready' }
    if (-not $server.lifecycle.starts_at_boot) { throw 'Automatic startup state missing' }
    $job = Join-Path $data 'server/job.toml'
    Run-Owner ('--plain agent stop --job "' + $job + '"') 'stop' | Out-Null
    Start-Sleep -Seconds 2
    $status = (Run-Owner '--json status' 'stopped') | ConvertFrom-Json
    if (($status.processes | Where-Object role -eq 'server_agent').lifecycle.phase -ne 'stopped') { throw 'Ordinary owner could not stop the server job' }
    Run-Owner ('--plain agent start --job "' + $job + '"') 'start' | Out-Null
    Start-Sleep -Seconds 2
    foreach ($name in @('FrpShClient','FrpShServer')) {
        $service = Get-CimInstance Win32_Service -Filter ("Name='" + $name + "'")
        if ($service.StartName -ne ('NT SERVICE\' + $name) -or $service.StartMode -ne 'Auto') { throw 'Service must use a virtual account and automatic startup' }
        Stop-Service $name
        (Get-Service $name).WaitForStatus('Stopped',[TimeSpan]::FromSeconds(15))
    }
    & $exe --json doctor
    if ($LASTEXITCODE -eq 0) { throw 'Unlisted administrator obtained helper IPC access' }
    Write-Output 'SCM virtual-account startup, owner status, persisted controls and shutdown verified'
} finally {
    foreach ($name in @('FrpShClient','FrpShServer','FrpShNetwork')) { Stop-Service $name -ErrorAction SilentlyContinue; & sc.exe delete $name | Out-Null }
    Remove-LocalUser -Name $user -ErrorAction SilentlyContinue
    foreach ($path in @($destination,$data)) {
        $full = [IO.Path]::GetFullPath($path)
        if ($full -notin @([IO.Path]::GetFullPath($destination),[IO.Path]::GetFullPath($data))) { throw 'Unsafe cleanup target' }
        Remove-Item -LiteralPath $full -Recurse -Force -ErrorAction SilentlyContinue
    }
}
