# Exercise installer functions only: no elevation, services, or machine changes.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$path = Join-Path $root 'web/docs/public/install.ps1'
$bytes = [IO.File]::ReadAllBytes($path)
if (@($bytes | Where-Object { $_ -gt 127 }).Count) { throw 'Installer must be ASCII-safe for irm and -File' }
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
if ($errors) { throw ($errors | Out-String) }
$pathFunction = $ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FrpShPath'}, $true)
. ([scriptblock]::Create($pathFunction.Extent.Text))
$destination = 'C:\Program Files\frp-sh'
$legacy = 'C:\Users\Example\AppData\Local\frp-sh'
$current = $legacy + ';C:\Windows;"C:\PROGRAM FILES\frp-sh\";;' + $destination
$updated = Get-FrpShPath $current $destination
if ($updated -ne ($destination + ';' + $legacy + ';C:\Windows')) { throw 'New installation does not take precedence or PATH entries were lost' }
if ((Get-FrpShPath $updated $destination) -ne $updated) { throw 'Repeated installation duplicates PATH entries' }
if ((Get-FrpShPath '' $destination) -ne $destination) { throw 'Empty PATH handling failed' }
$text = [IO.File]::ReadAllText($path)
$null = [Management.Automation.Language.Parser]::ParseInput($text, [ref]$tokens, [ref]$errors)
if ($errors) { throw ($errors | Out-String) }
$messageFunction = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Message' }, $true)
. ([scriptblock]::Create($messageFunction.Extent.Text))
function Write-Host { param($Object) $script:messageOutput = $Object }
$Lang = 'zh-CN'
Message 'Test' '\u4e2d\u6587'
if ($script:messageOutput -ne (-join @([char]0x4e2d, [char]0x6587))) { throw 'Chinese output decoding failed' }
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Download-Verified' }, $true)
. ([scriptblock]::Create($function.Extent.Text))
$base = 'https://example.invalid/release'
$script:payload = [Text.Encoding]::UTF8.GetBytes('installer regression fixture')
$sha = [Security.Cryptography.SHA256]::Create()
$script:checksum = ([BitConverter]::ToString($sha.ComputeHash($script:payload))).Replace('-','').ToLowerInvariant()
$sha.Dispose()
function Invoke-WebRequest {
    param($Uri, $OutFile, [switch]$UseBasicParsing)
    if (-not $OutFile) { throw 'Download must use raw file bytes, not response Content' }
    if ($Uri.EndsWith('.sha256')) {
        [IO.File]::WriteAllBytes($OutFile, [Text.Encoding]::UTF8.GetBytes($script:checksum + "  fixture.exe`r`n"))
    } else { [IO.File]::WriteAllBytes($OutFile, $script:payload) }
}
$temp = Join-Path ([IO.Path]::GetTempPath()) ('frpsh-checksum-' + [guid]::NewGuid().ToString('N'))
try {
    Download-Verified 'fixture.exe' $temp
    foreach ($bad in @('invalid', ('0' * 64))) {
        $script:checksum = $bad
        $rejected = $false
        try { Download-Verified 'fixture.exe' $temp } catch { $rejected = $_.Exception.Message -like 'Checksum mismatch:*' }
        if (-not $rejected) { throw 'Invalid checksum was accepted' }
    }
    Write-Output ('Installer encoding and checksum regressions passed on PowerShell ' + $PSVersionTable.PSVersion)
} finally {
    Remove-Item -LiteralPath $temp,($temp + '.sha256') -Force -ErrorAction SilentlyContinue
}
