# Destructive setup is limited to an ephemeral GitHub Actions runner.
$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') { throw 'Run only on an ephemeral Windows CI runner' }
$destination=Join-Path $env:ProgramFiles 'frp-sh-ci'
$user='FrpshCiUser'
$secret=[guid]::NewGuid().ToString('N')+'aA!9'
$password=ConvertTo-SecureString $secret -AsPlainText -Force
$account=New-LocalUser -Name $user -Password $password -AccountNeverExpires
$secret=$null
$results=Join-Path $env:RUNNER_TEMP 'frpsh-helper-results'
New-Item -ItemType Directory -Force -Path $destination,$results | Out-Null
try {
    & icacls.exe $destination /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX' | Out-Null
    Copy-Item target/debug/frp-sh.exe,target/debug/frp-sh-net.exe -Destination $destination
    $zip=Join-Path $results 'wintun.zip'
    Invoke-WebRequest 'https://www.wintun.net/builds/wintun-0.14.1.zip' -OutFile $zip
    Expand-Archive $zip -DestinationPath $results
    $dll=Join-Path $results 'wintun/bin/amd64/wintun.dll'
    if ((Get-AuthenticodeSignature $dll).Status -ne 'Valid') {throw 'Driver signature failed'}
    Copy-Item $dll -Destination $destination
    [IO.File]::WriteAllText((Join-Path $destination 'helper.toml'),('allowed_sid = "'+$account.SID.Value+'"'),(New-Object Text.UTF8Encoding($false)))
    & sc.exe create FrpShNetwork binPath= ('"'+(Join-Path $destination 'frp-sh-net.exe')+'"') start= demand | Out-Null
    if ($LASTEXITCODE -ne 0) {throw 'Service creation failed'}
    Start-Service FrpShNetwork
    Start-Sleep -Seconds 2
    & icacls.exe $results /grant ('*'+$account.SID.Value+':(OI)(CI)M') | Out-Null
    if ($LASTEXITCODE -ne 0) {throw 'Test output directory ACL failed'}
    $credential=New-Object Management.Automation.PSCredential("$env:COMPUTERNAME\$user",$password)
    $exe=Join-Path $destination 'frp-sh.exe'
    $child=Start-Process -FilePath $exe -ArgumentList '--lang en --json doctor --network-test' -Credential $credential -LoadUserProfile -WorkingDirectory $destination -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput (Join-Path $results 'out.json') -RedirectStandardError (Join-Path $results 'err.json')
    Get-Content (Join-Path $results 'out.json'),(Join-Path $results 'err.json')
    if ($child.ExitCode -ne 0) {throw 'Ordinary-user network test failed'}
    Start-Sleep -Seconds 2
    if (Get-NetAdapter -Name frp0 -ErrorAction SilentlyContinue) {throw 'Virtual adapter leaked'}
    # The CI administrator is not the configured ordinary user.
    & $exe --json doctor
    if ($LASTEXITCODE -eq 0) {throw 'Unlisted account obtained IPC access'}
} finally {
    Stop-Service FrpShNetwork -ErrorAction SilentlyContinue
    & sc.exe delete FrpShNetwork | Out-Null
    Remove-LocalUser -Name $user -ErrorAction SilentlyContinue
    $resolved=[IO.Path]::GetFullPath($destination)
    if ($resolved -ne (Join-Path $env:ProgramFiles 'frp-sh-ci')) {throw 'Unexpected cleanup target'}
    Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
}
