[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'update-desktop.ps1') -FunctionsOnly

function Assert-Equal($Actual, $Expected, [string]$Message) {
    if ($Actual -cne $Expected) { throw "$Message (expected '$Expected', got '$Actual')" }
}
function Assert-Throws([scriptblock]$Action, [string]$Message) {
    $threw = $false
    try { & $Action } catch { $threw = $true }
    if (-not $threw) { throw $Message }
}
function New-Release([string]$Tag, [string]$Commit) {
    [pscustomobject]@{
        tag_name = $Tag; target_commitish = $Commit; draft = $false; prerelease = $true
        assets = @([pscustomobject]@{
            name = 'RusticGameEngine-Setup-0.1.0-x64-test.exe'; size = 3
            digest = 'sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'
            browser_download_url = "https://github.com/Rustic-Game-Engine/engine/releases/download/$Tag/RusticGameEngine-Setup-0.1.0-x64-test.exe"
        })
    }
}

$temp = Join-Path ([IO.Path]::GetTempPath()) ('rustic-update-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temp | Out-Null
try {
    $script:CacheDirectory = $temp
    $commit = 'a' * 40
    $current = New-Release 'v0.9-Pre-Alpha' $commit
    $next = New-Release 'v1.0-Pre-Alpha' ('b' * 40)
    $releases = @($current, $next)
    Assert-Equal (Select-DesktopUpdate $releases $commit).browser_download_url $next.assets[0].browser_download_url 'Select newer prerelease across rollover'
    Assert-Equal (Select-DesktopUpdate $releases ('b' * 40)) $null 'Never reinstall current release'
    Assert-Throws { Select-DesktopUpdate $releases ('c' * 40) } 'Unpublished source builds must not downgrade'
    Assert-Throws { Select-DesktopUpdate $releases '../bad' } 'Invalid installed identity must fail'
    Assert-Equal ((Get-ReleaseVersion 'v1.10-Pre-Alpha') -gt (Get-ReleaseVersion 'v1.9-Pre-Alpha')) $true 'Compare numeric versions'
    Assert-Equal ((Get-ReleaseVersion 'v1.0.0') -gt (Get-ReleaseVersion 'v1.0-Pre-Alpha')) $true 'Stable follows prerelease'
    Assert-Equal (Get-ReleaseVersion 'unrelated-tag') $null 'Ignore other release channels'
    $next.draft = $true
    Assert-Equal (Select-DesktopUpdate $releases $commit) $null 'Ignore draft releases'
    $next.draft = $false
    $asset = $next.assets[0]
    $digest = $asset.digest
    $asset.digest = $null
    Assert-Equal (Select-DesktopUpdate $releases $commit) $null 'Require GitHub checksum'
    $asset.digest = $digest
    $next.assets = @($asset, $asset)
    Assert-Equal (Select-DesktopUpdate $releases $commit) $null 'Reject ambiguous installers'
    $next.assets = @($asset)
    $url = $asset.browser_download_url
    $asset.browser_download_url = 'https://example.com/installer.exe'
    Assert-Throws { Select-DesktopUpdate $releases $commit } 'Reject foreign download URL'
    $asset.browser_download_url = $url

    # No network is used. Match Invoke-RestMethod's non-enumerated array response.
    $script:Pages = 0
    function Invoke-RestMethod {
        param($Uri, $Headers, $TimeoutSec)
        $script:Pages++
        if ($script:Pages -eq 1) { return ,(@($current) * 100) }
        return ,@($next)
    }
    Assert-Equal @(Get-PublishedReleases).Count 101 'Read releases beyond the first API page'
    Assert-Equal $script:Pages 2 'Stop pagination on the final page'

    function Invoke-WebRequest {
        param($Uri, $Headers, $OutFile, $TimeoutSec, [switch]$UseBasicParsing)
        [IO.File]::WriteAllText($OutFile, 'abc', [Text.Encoding]::ASCII)
    }
    $installer = Save-VerifiedInstaller $asset $temp
    Assert-Equal (Test-Path -LiteralPath $installer) $true 'Verified download becomes an installer'
    Remove-Item -LiteralPath $installer
    $asset.digest = 'sha256:' + ('0' * 64)
    Assert-Throws { Save-VerifiedInstaller $asset $temp } 'Reject checksum mismatch'
    Assert-Equal (Test-Path -LiteralPath $installer) $false 'Corrupt installer is never staged'
    Assert-Equal (Test-Path -LiteralPath ($installer + '.partial')) $false 'Clean up partial files'
    $asset.digest = $digest
    $asset.size = 4
    Assert-Throws { Save-VerifiedInstaller $asset $temp } 'Reject truncated installer'
    $asset.size = 3
    function Invoke-WebRequest { throw 'Network unavailable' }
    Assert-Throws { Save-VerifiedInstaller $asset $temp } 'Network failure cannot stage an update'

    function Get-Process {
        param($Name, $ErrorAction)
        [pscustomobject]@{ Path = (Join-Path $temp 'rustic-editor.exe') }
    }
    Assert-Equal (Test-RusticRunning $temp) $true 'Wait for open editor'
    Assert-Equal (Test-RusticRunning (Join-Path $temp 'another-install')) $false 'Other installations do not block'
    function Get-Process { param($Name, $ErrorAction); [pscustomobject]@{ Path = $null } }
    Assert-Equal (Test-RusticRunning $temp) $true 'Inaccessible process must block installation'

    # Exercise the download -> wait -> install path without starting any executable.
    Set-Content -LiteralPath (Join-Path $temp 'build-commit.txt') -Value $commit
    Set-Content -LiteralPath (Join-Path $temp 'install-scope.txt') -Value 'currentuser'
    function Get-PublishedReleases { $releases }
    function Invoke-WebRequest {
        param($Uri, $Headers, $OutFile, $TimeoutSec, [switch]$UseBasicParsing)
        [IO.File]::WriteAllText($OutFile, 'abc', [Text.Encoding]::ASCII)
    }
    $script:Waits = 0
    $script:Launches = 0
    $script:SetupExitCode = 0
    function Test-RusticRunning { param($Directory); return $script:Waits -eq 0 }
    function Start-Sleep { param($Seconds); $script:Waits++ }
    function Start-Process {
        param($FilePath, $ArgumentList, [switch]$PassThru, [switch]$Wait)
        Assert-Equal ($script:Waits -gt 0) $true 'Install only after editor closes'
        Assert-Equal ($ArgumentList -contains '/NOCLOSEAPPLICATIONS') $true 'Never force close applications'
        Assert-Equal ($ArgumentList -contains '/NORESTART') $true 'Never reboot'
        Assert-Equal ($ArgumentList -contains '/currentuser') $true 'Preserve installation scope'
        Assert-Equal ($ArgumentList -contains ('/DIR="' + $temp + '"')) $true 'Preserve installation directory'
        $script:Launches++
        [pscustomobject]@{ ExitCode = $script:SetupExitCode }
    }
    Assert-Equal (Invoke-DesktopUpdateCheck $temp $temp) $true 'Successful background update'
    Assert-Equal $script:Launches 1 'Launch Setup exactly once'
    Assert-Equal (Test-Path -LiteralPath $installer) $false 'Remove installer after success'
    $script:SetupExitCode = 5
    Assert-Throws { Invoke-DesktopUpdateCheck $temp $temp } 'Report installer failure for retry'
    $script:Launches = 0
    $asset.digest = 'sha256:' + ('0' * 64)
    Assert-Throws { Invoke-DesktopUpdateCheck $temp $temp } 'Checksum failure propagates before installation'
    Assert-Equal $script:Launches 0 'Never execute corrupt downloads'
    Write-Host 'Desktop updater regression tests passed.'
} finally { Remove-Item -LiteralPath $temp -Recurse -Force }
