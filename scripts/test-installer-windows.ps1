# Exercise installer functions only: no elevation, services, or machine changes.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$path = Join-Path $root 'web/docs/public/install.ps1'
$bytes = [IO.File]::ReadAllBytes($path)
if ($bytes[0] -ne 0xEF -or $bytes[1] -ne 0xBB -or $bytes[2] -ne 0xBF) { throw 'Installer needs UTF-8 BOM for Windows PowerShell -File' }
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
if ($errors) { throw ($errors | Out-String) }
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
