[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot '../apps/editor/src/project_build/ensure-inno-setup.ps1') -FunctionsOnly
function Assert-Equal($Actual, $Expected, [string]$Message) {
    if ($Actual -cne $Expected) { throw "$Message (expected '$Expected', got '$Actual')" }
}
function Assert-Throws([scriptblock]$Action, [string]$Message) {
    $threw = $false
    try { & $Action | Out-Null } catch { $threw = $true }
    if (-not $threw) { throw $Message }
}
$temp = Join-Path ([IO.Path]::GetTempPath()) ('rustic-inno-test-' + [guid]::NewGuid())
$previousLocal = $env:LOCALAPPDATA
try {
    New-Item -ItemType Directory -Path $temp | Out-Null
    $custom = Join-Path $temp 'custom compiler.exe'
    [IO.File]::WriteAllText($custom, '')
    Assert-Equal (Find-InnoCompiler $custom) $custom 'Resolve a custom compiler with spaces'
    Assert-Throws { Find-InnoCompiler (Join-Path $temp 'missing.exe') } 'Reject missing custom paths'
    Assert-Throws { Find-InnoCompiler 'relative/ISCC.exe' } 'Reject relative custom paths'
    Assert-Equal (Get-InnoUserPath '' 'C:\Inno') 'C:\Inno' 'Initialize an empty user PATH'
    Assert-Equal (Get-InnoUserPath 'C:\Other;' 'C:\Inno') 'C:\Other;C:\Inno' 'Preserve existing PATH entries'
    Assert-Equal (Get-InnoUserPath 'C:\Other;C:\INNO\' 'C:\Inno') 'C:\Other;C:\INNO\' 'Avoid case-insensitive duplicates'
    Assert-Equal (Get-InnoUserPath '"C:\Inno";C:\Other' 'C:\Inno') '"C:\Inno";C:\Other' 'Recognize quoted PATH entries'

    # All external operations are mocked; no download, installation or user PATH changes.
    $env:LOCALAPPDATA = $temp
    $script:Downloads = 0
    $script:Installs = 0
    $script:Signature = 'Valid'
    $script:Signer = 'CN=Jordan Russell, O=Jordan Russell'
    $script:ExitCode = 0
    function Invoke-WebRequest {
        param($Uri, $OutFile, $TimeoutSec, [switch]$UseBasicParsing)
        $script:Downloads++
        [IO.File]::WriteAllText($OutFile, 'mock installer')
    }
    function Get-AuthenticodeSignature {
        param($LiteralPath)
        [pscustomobject]@{ Status = $script:Signature; SignerCertificate = [pscustomobject]@{ Subject = $script:Signer } }
    }
    function Start-Process {
        param($FilePath, $ArgumentList, $WindowStyle, [switch]$Wait, [switch]$PassThru)
        $script:Installs++
        $root = Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6'
        Assert-Equal $ArgumentList[-1] "/DIR=`"$root`"" 'Quote the installation directory'
        New-Item -ItemType Directory -Force -Path $root | Out-Null
        [IO.File]::WriteAllText((Join-Path $root 'ISCC.exe'), '')
        [pscustomobject]@{ ExitCode = $script:ExitCode }
    }
    $script:Signature = 'NotTrusted'
    Assert-Throws { Install-InnoCompiler } 'Reject untrusted downloads'
    Assert-Equal $script:Installs 0 'Untrusted installer must never execute'
    $script:Signature = 'Valid'
    $script:Signer = 'CN=Unrelated Publisher'
    Assert-Throws { Install-InnoCompiler } 'Reject a different signed publisher'
    Assert-Equal $script:Installs 0 'Foreign installer must never execute'
    $script:Signer = 'CN=Jordan Russell, O=Jordan Russell'
    $script:ExitCode = 5
    Assert-Throws { Install-InnoCompiler } 'Report installation failures'
    $script:ExitCode = 0
    $installed = Install-InnoCompiler
    Assert-Equal (Test-Path -LiteralPath $installed) $true 'Successful installation resolves compiler'
    function Get-Command { param($Name, $CommandType, $ErrorAction); return $null }
    function Get-ItemProperty { param($LiteralPath, $Name, $ErrorAction); return $null }
    Assert-Equal (Find-InnoCompiler 'ISCC.exe') $installed 'Find an installation outside the inherited PATH'
    function Invoke-WebRequest { throw 'Network unavailable' }
    Assert-Throws { Install-InnoCompiler } 'Report failed downloads'

    $originalFind = ${function:Find-InnoCompiler}
    $script:Resolved = $installed
    $script:Added = $null
    $script:AutomaticInstalls = 0
    function Find-InnoCompiler { param($Compiler); return $script:Resolved }
    function Install-InnoCompiler { $script:AutomaticInstalls++; return $installed }
    function Add-InnoUserPath { param($Compiler); $script:Added = $Compiler }
    Assert-Equal (Ensure-InnoCompiler 'ISCC.exe') $installed 'Reuse an existing installation'
    Assert-Equal $script:AutomaticInstalls 0 'Do not reinstall an existing compiler'
    Assert-Equal $script:Added $installed 'Add existing installation to PATH'
    $script:Resolved = $null
    Assert-Equal (Ensure-InnoCompiler '') $installed 'Install when default compiler is missing'
    Assert-Equal $script:AutomaticInstalls 1 'Install missing compiler exactly once'
    Set-Item Function:Find-InnoCompiler $originalFind
    Assert-Throws { Ensure-InnoCompiler (Join-Path $temp 'missing.exe') } 'Invalid explicit path must not trigger installation'
    Assert-Equal $script:AutomaticInstalls 1 'Preserve explicit path errors'
    Write-Host 'Inno Setup configuration tests passed.'
} finally {
    $env:LOCALAPPDATA = $previousLocal
    Remove-Item -LiteralPath $temp -Recurse -Force
}
