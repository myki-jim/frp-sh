$ErrorActionPreference = 'Stop'
$installSpec = [Console]::In.ReadToEnd() | ConvertFrom-Json
$program = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'frp-sh/frp-sh.exe'
if ([IO.Path]::GetFullPath($installSpec.executable) -ne [IO.Path]::GetFullPath($program)) { throw 'Install the verified client into Program Files before registering a service' }
$owner = [Security.Principal.SecurityIdentifier]::new($installSpec.owner_sid)
$serviceName = if ($installSpec.server) { 'FrpShServer' } else { 'FrpShClient' }
$role = if ($installSpec.server) { 'server' } else { 'client' }
$root = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'frp-sh'
$directory = Join-Path $root $role
$ownerFile = Join-Path $root ($role + '-owner.txt')
$jobPath = Join-Path $directory 'job.toml'
$configPath = Join-Path $directory 'config.toml'
$serviceAccount = 'NT SERVICE\' + $serviceName
function Ensure-Directory([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        $item = Get-Item -LiteralPath $Path -Force
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unsafe service directory' }
    } else {
        $acl = [Security.AccessControl.DirectorySecurity]::new()
        $acl.SetAccessRuleProtection($true,$false)
        $acl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
        foreach ($sid in @('S-1-5-18','S-1-5-32-544')) {
            $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
        }
        # Windows PowerShell 5.1: assign the private ACL at directory creation.
        [IO.Directory]::CreateDirectory($Path,$acl) | Out-Null
    }
}
function Require-TrustedDirectory([string]$Path) {
    $acl = Get-Acl -LiteralPath $Path
    $sid = $acl.GetOwner([Security.Principal.SecurityIdentifier]).Value
    if ($sid -notin @('S-1-5-18','S-1-5-32-544')) { throw 'Service root must be owned by SYSTEM or Administrators' }
    $writes = [Security.AccessControl.FileSystemRights]::Write -bor [Security.AccessControl.FileSystemRights]::Delete -bor [Security.AccessControl.FileSystemRights]::DeleteSubdirectoriesAndFiles -bor [Security.AccessControl.FileSystemRights]::ChangePermissions -bor [Security.AccessControl.FileSystemRights]::TakeOwnership
    foreach ($rule in $acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -eq 'Allow' -and ($rule.FileSystemRights -band $writes) -and $rule.IdentityReference.Value -notin @('S-1-5-18','S-1-5-32-544')) { throw 'Service root is writable by an untrusted account' }
    }
}
function Protect-Directory([string]$Path,[string]$ServiceSid,[bool]$UserAccess) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true,$false)
    $admin = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
    $acl.SetOwner($admin)
    foreach ($sid in @('S-1-5-18','S-1-5-32-544')) {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))
    }
    if ($UserAccess) {
        foreach ($sid in @($owner.Value,$ServiceSid)) {
            $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'Modify','ContainerInherit,ObjectInherit','None','Allow'))
        }
    } else {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-32-545'),'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'))
    }
    Set-Acl -LiteralPath $Path -AclObject $acl
}
function Safe-File([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        $item = Get-Item -LiteralPath $Path -Force
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unsafe service file' }
    }
}
function Sc-Checked([string[]]$Arguments) {
    & (Join-Path ([Environment]::GetFolderPath('System')) 'sc.exe') @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Service registration operation failed' }
}
Ensure-Directory $root
Require-TrustedDirectory $root
Safe-File $ownerFile
if ((Test-Path -LiteralPath $ownerFile) -and [IO.File]::ReadAllText($ownerFile).Trim() -ne $owner.Value) { throw 'This service belongs to another installation account' }
Protect-Directory $root '' $false
$binaryPath = '"' + $program + '" --plain agent service --job "' + $jobPath + '"'
$existing = Get-CimInstance Win32_Service -Filter ("Name='" + $serviceName + "'")
if ($existing -and $existing.PathName -ne $binaryPath) { throw 'Existing service has an unexpected executable configuration' }
$helperPath = Join-Path (Split-Path $program) 'helper.toml'
$previousHelper = $null
if (-not $installSpec.server) {
    Safe-File $helperPath
    $previousHelper = [IO.File]::ReadAllText($helperPath)
    if ($previousHelper -notmatch ('(?m)^allowed_sid\s*=\s*"' + [regex]::Escape($owner.Value) + '"\s*$')) { throw 'Network helper belongs to another account; repair its installation first' }
    if (-not (Get-Service FrpShNetwork -ErrorAction SilentlyContinue)) { throw 'Install the network helper first' }
}
Ensure-Directory $directory
foreach ($path in @($jobPath,$configPath)) { Safe-File $path }
$previous = @{}
$stage = Join-Path $root ('.install-' + [guid]::NewGuid().ToString('N'))
Ensure-Directory $stage
# Prepare new files in an administrator-only directory. Never write through a
# mutable user-owned destination or read backup contents with elevated rights.
[IO.File]::WriteAllText((Join-Path $stage 'config.new'),$installSpec.config,[Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $stage 'job.new'),$installSpec.job,[Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $stage 'owner.new'),$owner.Value,[Text.UTF8Encoding]::new($false))
$written = @()
$wasRunning = $existing -and $existing.State -eq 'Running'
$created = $false
$cleanupSafe = $false
try {
    if ($existing) { Stop-Service $serviceName -ErrorAction Stop }
    else {
        $arguments = @('create',$serviceName,'binPath=',$binaryPath,'start=','delayed-auto','obj=',$serviceAccount,'DisplayName=',('frp-sh ' + $role))
        if (-not $installSpec.server) { $arguments += @('depend=','FrpShNetwork') }
        Sc-Checked $arguments
        $created = $true
    }
    $serviceSid = ([Security.Principal.NTAccount]::new($serviceAccount)).Translate([Security.Principal.SecurityIdentifier]).Value
    Protect-Directory $directory $serviceSid $true
    foreach ($name in @('config','job')) {
        $acl = [Security.AccessControl.FileSecurity]::new()
        $acl.SetAccessRuleProtection($true,$false)
        $acl.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
        foreach ($sid in @('S-1-5-18','S-1-5-32-544',$owner.Value,$serviceSid)) {
            $rights = if ($sid -in @($owner.Value,$serviceSid)) { 'Modify' } else { 'FullControl' }
            $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),$rights,'Allow'))
        }
        Set-Acl -LiteralPath (Join-Path $stage ($name + '.new')) -AclObject $acl
    }
    foreach ($item in @(@('config',$configPath),@('job',$jobPath),@('owner',$ownerFile))) {
        $name = $item[0]; $path = $item[1]
        if (Test-Path -LiteralPath $path) {
            $backup = Join-Path $stage ($name + '.previous')
            [IO.File]::Move($path,$backup)
            $previous[$path] = $backup
        }
        [IO.File]::Move((Join-Path $stage ($name + '.new')),$path)
        $written += $path
    }
    if (-not $installSpec.server) {
        $policy = [regex]::Replace($previousHelper,'(?m)^service_sid\s*=.*\r?\n?','')
        [IO.File]::WriteAllText($helperPath,($policy.TrimEnd() + "`nservice_sid = `"" + $serviceSid + "`"`n"),[Text.UTF8Encoding]::new($false))
        Restart-Service FrpShNetwork
    }
    Sc-Checked @('failure',$serviceName,'reset=','86400','actions=','restart/3000/restart/10000/restart/30000')
    Sc-Checked @('privs',$serviceName,'SeChangeNotifyPrivilege')
    Start-Service $serviceName
    (Get-Service $serviceName).WaitForStatus('Running',[TimeSpan]::FromSeconds(15))
    Write-Output ('Installed ' + $serviceName + '; job: ' + $jobPath)
    $cleanupSafe = $true
} catch {
    Stop-Service $serviceName -ErrorAction SilentlyContinue
    if ($created) { & (Join-Path ([Environment]::GetFolderPath('System')) 'sc.exe') delete $serviceName | Out-Null }
    foreach ($path in $written) { Remove-Item -LiteralPath $path -ErrorAction SilentlyContinue }
    foreach ($path in $previous.Keys) {
        try { [IO.File]::Move($previous[$path],$path) } catch { throw ('Installation failed; restore protected backups from ' + $stage) }
    }
    if ($null -ne $previousHelper) { [IO.File]::WriteAllText($helperPath,$previousHelper,[Text.UTF8Encoding]::new($false)); Restart-Service FrpShNetwork -ErrorAction SilentlyContinue }
    if ($wasRunning) { Start-Service $serviceName -ErrorAction SilentlyContinue }
    $cleanupSafe = $true
    throw 'Service installation failed; prior configuration was restored'
} finally {
    if ($cleanupSafe) {
    # No recursive cleanup of moved, potentially user-controlled file objects.
    foreach ($name in @('config.new','job.new','owner.new','config.previous','job.previous','owner.previous')) {
        $path = Join-Path $stage $name
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force
            if (-not $item.PSIsContainer) { Remove-Item -LiteralPath $path -ErrorAction SilentlyContinue }
        }
    }
    Remove-Item -LiteralPath $stage -ErrorAction SilentlyContinue
    }
}
