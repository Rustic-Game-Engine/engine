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
        $script:Downloads++
        [IO.File]::WriteAllText($OutFile, 'abc', [Text.Encoding]::ASCII)
    }
    $script:Downloads = 0
    $installer = Save-VerifiedInstaller $asset $temp
    Assert-Equal (Test-Path -LiteralPath $installer) $true 'Verified download becomes an installer'
    Assert-Equal (Save-VerifiedInstaller $asset $temp) $installer 'Reuse a verified cached installer'
    Assert-Equal $script:Downloads 1 'Retry must not download the installer again'
    [IO.File]::WriteAllText($installer, 'bad', [Text.Encoding]::ASCII)
    Save-VerifiedInstaller $asset $temp | Out-Null
    Assert-Equal $script:Downloads 2 'Recheck the checksum and replace corrupt cached installers'
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
    function Get-Process { param($Name, $ErrorAction); [pscustomobject]@{ Path = $null; ProcessName = 'rustic-runtime'; Id = 123 } }
    Assert-Equal (Test-RusticRunning $temp) $true 'Inaccessible process must block installation'

    # MCP clients can keep several backends alive after all desktop windows close.
    $editor = [pscustomobject]@{ Path = (Join-Path $temp 'rustic-editor.exe'); ProcessName = 'rustic-editor'; Id = 1 }
    $backend1 = [pscustomobject]@{ Path = (Join-Path $temp 'rustic-agent-backend.exe'); ProcessName = 'rustic-agent-backend'; Id = 2 }
    $backend2 = [pscustomobject]@{ Path = $backend1.Path; ProcessName = 'rustic-agent-backend'; Id = 3 }
    $foreign = [pscustomobject]@{ Path = (Join-Path $temp 'other/rustic-agent-backend.exe'); ProcessName = 'rustic-agent-backend'; Id = 4 }
    $unknown = [pscustomobject]@{ Path = $null; ProcessName = 'rustic-agent-backend'; Id = 5 }
    $script:Processes = @($editor, $backend1, $backend2, $foreign)
    $script:Stopped = @()
    $script:FailStop = $false
    function Get-Process {
        param($Name, $ErrorAction)
        $script:Processes | Where-Object {
            $processName = $_.ProcessName
            @($Name | Where-Object { $processName -like $_ }).Count -gt 0
        }
    }
    function Stop-Process {
        param($InputObject, [switch]$Force, $ErrorAction)
        if ($script:FailStop) { throw 'Access denied' }
        $script:Stopped += $InputObject.Id
        $script:Processes = @($script:Processes | Where-Object { $_.Id -ne $InputObject.Id })
    }
    Stop-IdleAgentBackends $temp
    Assert-Equal $script:Stopped.Count 0 'Keep MCP available while an editor is open'
    foreach ($name in @('rustic-project-manager', 'rustic-runtime', 'rustic-asset-worker', 'rustic-luau-host', 'rustic-setup-helper')) {
        $worker = [pscustomobject]@{ Path = (Join-Path $temp ($name + '.exe')); ProcessName = $name; Id = 6 }
        $script:Processes = @($worker, $backend1)
        Stop-IdleAgentBackends $temp
        Assert-Equal $script:Stopped.Count 0 "Keep MCP available while $name is open"
    }
    $script:Processes = @($backend1, $backend2, $foreign)
    # No explicit close request is required after a normal desktop exit.
    Assert-Equal (Wait-DesktopExit $temp $temp $next.tag_name) $true 'Remaining backends cannot prevent installation'
    Assert-Equal ($script:Stopped -join ',') '2,3' 'Stop every backend from this installation'
    Assert-Equal $script:Processes[0].Id 4 'Leave other installations running'
    $script:Processes = @($unknown)
    Stop-IdleAgentBackends $temp
    Assert-Equal $script:Stopped.Count 2 'Never stop a backend with an unknown executable path'
    Assert-Equal (Test-RusticRunning $temp) $true 'An unknown backend still blocks Setup'
    $script:Processes = @($backend1, $editor)
    $editor.Path = $null
    Stop-IdleAgentBackends $temp
    Assert-Equal $script:Stopped.Count 2 'An inaccessible desktop process prevents backend cleanup'

    # A failed termination must keep blocking Setup and be retried on the next poll.
    $script:Processes = @($backend1, $backend2)
    $script:FailStop = $true
    Stop-IdleAgentBackends $temp
    Assert-Equal (Test-RusticRunning $temp) $true 'Failed termination still blocks Setup'
    Assert-Equal ((Get-Content -LiteralPath (Join-Path $temp 'update.log') -Raw) -match 'Access denied') $true 'Log termination failures'
    $script:CleanupPolls = 0
    function Start-Sleep {
        param($Seconds)
        $script:CleanupPolls++
        if ($script:CleanupPolls -gt 1) { throw 'Backends were not retried promptly' }
        $script:FailStop = $false
        # Simulate a client starting another backend between updater polls.
        $script:Processes += [pscustomobject]@{ Path = $backend1.Path; ProcessName = 'rustic-agent-backend'; Id = 7 }
    }
    Assert-Equal (Wait-DesktopExit $temp $temp $next.tag_name) $true 'Retry failed and newly started backends on each poll'
    Assert-Equal ($script:Stopped -join ',') '2,3,2,3,7' 'Cleanup includes every remaining backend'
    Remove-Item Function:Stop-Process
    function Get-Process { param($Name, $ErrorAction); [pscustomobject]@{ Path = $null; ProcessName = 'rustic-runtime'; Id = 123 } }

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
    Assert-Equal (Invoke-DesktopUpdateCheck $temp $temp).Installed $true 'Successful background update'
    Assert-Equal $script:Launches 1 'Launch Setup exactly once'
    Assert-Equal (Test-Path -LiteralPath $installer) $false 'Remove installer after success'
    Assert-Equal (Test-Path -LiteralPath (Join-Path $temp 'pending-update.txt')) $false 'Clear pending UI status after success'
    $script:SetupExitCode = 5
    Set-Content -LiteralPath (Join-Path $temp 'install-request.txt') -Value $next.tag_name
    Set-Content -LiteralPath (Join-Path $temp 'resume-failure.txt') -Value 'must not replay'
    Assert-Throws { Invoke-DesktopUpdateCheck $temp $temp } 'Report installer failure for retry'
    Assert-Equal (Test-Path -LiteralPath (Join-Path $temp 'install-request.txt')) $false 'Failed Setup must not close the next desktop session'
    Assert-Equal (Test-Path -LiteralPath (Join-Path $temp 'resume-failure.txt')) $false 'Failed Setup clears restart records'
    $pending = Get-Content -LiteralPath (Join-Path $temp 'pending-update.txt')
    Assert-Equal $pending[0] $commit 'UI status identifies the installed build'
    Assert-Equal $pending[1] $next.tag_name 'UI status identifies the verified release'
    $script:Launches = 0
    $asset.digest = 'sha256:' + ('0' * 64)
    Assert-Throws { Invoke-DesktopUpdateCheck $temp $temp } 'Checksum failure propagates before installation'
    Assert-Equal $script:Launches 0 'Never execute corrupt downloads'
    Assert-Equal (Test-Path -LiteralPath (Join-Path $temp 'pending-update.txt')) $false 'Only verified installers are advertised as ready'

    # A desktop left open must discover releases newer than its dismissed update.
    $asset.digest = $digest
    $script:Waits = 0
    function Test-RusticRunning { param($Directory); return $true }
    $refresh = Invoke-DesktopUpdateCheck $temp $temp
    Assert-Equal $refresh.Refresh $true 'Return to release discovery while a verified update waits'
    Assert-Equal $script:Waits 3600 'Refresh pending releases hourly without busy polling'
    Assert-Equal $script:Launches 0 'Refreshing must not start Setup while Rustic is open'
    Assert-Equal (Get-Content -LiteralPath (Join-Path $temp 'pending-update.txt'))[1] $next.tag_name 'Keep verified update visible during refresh'
    $newest = New-Release 'v1.1-Pre-Alpha' ('c' * 40)
    $releases = @($current, $next, $newest)
    $script:SetupExitCode = 0
    function Test-RusticRunning { param($Directory); return $false }
    Assert-Equal (Invoke-DesktopUpdateCheck $temp $temp).Tag $newest.tag_name 'Discover and install a newer release after refresh'

    # A remaining game/worker cannot keep closed desktop windows unavailable.
    $script:Waits = 0
    function Test-RusticRunning { param($Directory); return $script:Waits -lt 122 }
    $script:DeferredRestores = 0
    function Start-Process {
        param($FilePath, $ArgumentList, $WorkingDirectory)
        Assert-Equal $FilePath (Join-Path $temp 'rustic-project-manager.exe') 'Deferral restores the installed desktop, never Setup'
        Assert-Equal ($script:Waits -ge 119) $true 'Allow graceful closure before deferring'
        $script:DeferredRestores++
    }
    Set-Content -LiteralPath (Join-Path $temp 'install-request.txt') -Value "$($next.tag_name)`n$([guid]::NewGuid())"
    Set-Content -LiteralPath (Join-Path $temp 'pending-update.txt') -Value "$commit`n$($next.tag_name)"
    Set-Content -LiteralPath (Join-Path $temp 'resume-deferred.txt') -Value "$($next.tag_name)`nproject-manager"
    Wait-DesktopExit $temp $temp $next.tag_name
    Assert-Equal $script:DeferredRestores 1 'A blocked update restores closed windows exactly once'
    Assert-Equal (Test-Path -LiteralPath (Join-Path $temp 'install-request.txt')) $false 'Deferral cancels the shared close request'
    Assert-Equal (Test-Path -LiteralPath (Join-Path $temp 'pending-update.txt')) $true 'Deferral keeps the Update button available'
    Assert-Equal ((Get-Content -LiteralPath (Join-Path $temp 'update.log') -Raw) -match 'rustic-runtime \(PID 123\)') $true 'Log the process delaying installation'

    # Only sessions closed for this successful update are restored. Preserve spaces/Unicode.
    $project = Join-Path $temp ('M' + [char]0x0101 + 'ori game project')
    New-Item -ItemType Directory -Path $project | Out-Null
    $utf8 = [Text.UTF8Encoding]::new($false)
    [IO.File]::WriteAllText((Join-Path $temp 'resume-1.txt'), "$($next.tag_name)`nproject-manager`n", $utf8)
    [IO.File]::WriteAllText((Join-Path $temp 'resume-2.txt'), "$($next.tag_name)`neditor`n$project`n", $utf8)
    [IO.File]::WriteAllText((Join-Path $temp 'resume-3.txt'), "v0.1-Pre-Alpha`nproject-manager`n", $utf8)
    [IO.File]::WriteAllText((Join-Path $temp 'resume-4.txt'), "$($next.tag_name)`narbitrary-executable`n", $utf8)
    $script:Restored = @()
    function Start-Process {
        param($FilePath, $ArgumentList, $WorkingDirectory)
        $script:Restored += [pscustomobject]@{ Path = $FilePath; Arguments = $ArgumentList }
        Assert-Equal $WorkingDirectory $temp 'Restore using the installed application directory'
    }
    Restart-DesktopSessions $temp $temp $next.tag_name
    Assert-Equal $script:Restored.Count 2 'Restore only matching supported desktop sessions'
    Assert-Equal $script:Restored[0].Path (Join-Path $temp 'rustic-project-manager.exe') 'Reopen latest Project Manager'
    Assert-Equal $script:Restored[1].Path (Join-Path $temp 'rustic-editor.exe') 'Reopen latest editor'
    Assert-Equal $script:Restored[1].Arguments ('--project "' + $project + '"') 'Reopen the exact project with quoted Unicode/spaces'
    Assert-Equal @(Get-ChildItem -LiteralPath $temp -Filter 'resume-*.txt').Count 0 'Consume session records exactly once'
    Restart-DesktopSessions $temp $temp $next.tag_name
    Assert-Equal $script:Restored.Count 2 'Ordinary closed-app updates do not reopen applications'
    Write-Host 'Desktop updater regression tests passed.'
} finally { Remove-Item -LiteralPath $temp -Recurse -Force }
