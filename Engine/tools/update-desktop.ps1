[CmdletBinding()]
param(
    [string]$InstallDirectory = (Split-Path -Parent $PSScriptRoot),
    [string]$StateDirectory,
    [switch]$FunctionsOnly
)

$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1's per-chunk progress rendering makes downloads very slow,
# even though this worker has no visible console.
$ProgressPreference = 'SilentlyContinue'
Set-StrictMode -Version Latest
$script:Repository = 'Rustic-Game-Engine/engine'
$script:Headers = @{ 'User-Agent' = 'Rustic-Desktop-Updater'; Accept = 'application/vnd.github+json' }

function Get-ReleaseVersion([string]$Tag) {
    if ($Tag -match '^v(\d+)\.(\d+)(?:\.(\d+))?(-pre-alpha)?$') {
        $patch = if ($Matches[3]) { [int]$Matches[3] } else { 0 }
        # A stable release follows its pre-alpha at the same numeric version.
        $stable = if ($Matches[4]) { 0 } else { 1 }
        return [version]::new([int]$Matches[1], [int]$Matches[2], $patch, $stable)
    }
    return $null
}

function Get-PublishedReleases {
    $page = 1
    do {
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/$script:Repository/releases?per_page=100&page=$page" `
            -Headers $script:Headers -TimeoutSec 60
        $batch = @($response)
        foreach ($release in $batch) {
            if (-not $release.draft -and (Get-ReleaseVersion $release.tag_name)) { $release }
        }
        $page++
    } while ($batch.Count -eq 100)
}

function Select-DesktopUpdate($Releases, [string]$InstalledCommit) {
    if ($InstalledCommit -notmatch '^[0-9a-f]{40}$') { throw 'Installed build commit is invalid.' }
    $ordered = @($Releases | Where-Object { -not $_.draft -and (Get-ReleaseVersion $_.tag_name) } |
        Sort-Object { Get-ReleaseVersion $_.tag_name } -Descending)
    $installed = @($ordered | Where-Object { $_.target_commitish -eq $InstalledCommit })
    if ($installed.Count -eq 0) {
        throw 'This build has no published release yet; skipping updates to avoid a downgrade.'
    }
    $currentVersion = Get-ReleaseVersion $installed[0].tag_name
    foreach ($release in $ordered) {
        if ((Get-ReleaseVersion $release.tag_name) -le $currentVersion) { return $null }
        $assets = @($release.assets | Where-Object { $_.name -match '^RusticGameEngine-Setup-[\w.-]+-x64[\w.-]*\.exe$' })
        if ($assets.Count -ne 1) { continue }
        $asset = $assets[0]
        $digest = $asset.PSObject.Properties['digest']
        if (-not $digest -or $digest.Value -notmatch '^sha256:[0-9a-f]{64}$' -or $asset.size -le 0) { continue }
        # Asset names must remain a filename and downloads must belong to this repository.
        $expectedUrl = "https://github.com/$script:Repository/releases/download/$($release.tag_name)/$($asset.name)"
        if ($asset.browser_download_url -cne $expectedUrl) { throw 'Unexpected installer download URL.' }
        return $asset
    }
    return $null
}

function Save-VerifiedInstaller($Asset, [string]$Directory) {
    $partial = Join-Path $Directory ($Asset.name + '.partial')
    $installer = Join-Path $Directory $Asset.name
    $expected = $Asset.digest.Substring(7)
    if (Test-Path -LiteralPath $installer -PathType Leaf) {
        if ((Get-Item -LiteralPath $installer).Length -eq $Asset.size -and
            (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash -eq $expected) {
            Write-UpdateLog "Reusing verified installer $($Asset.name)."
            return $installer
        }
        Remove-Item -LiteralPath $installer -Force
    }
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $Asset.browser_download_url -Headers $script:Headers `
            -OutFile $partial -TimeoutSec 600 | Out-Null
        if ((Get-Item -LiteralPath $partial).Length -ne $Asset.size) { throw 'Installer size does not match GitHub.' }
        if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $expected) {
            throw 'Installer SHA-256 does not match GitHub.'
        }
        Move-Item -LiteralPath $partial -Destination $installer -Force
        return $installer
    } finally {
        Remove-Item -LiteralPath $partial -Force -ErrorAction SilentlyContinue
    }
}

function Test-RusticRunning([string]$Directory) {
    $names = @('rustic-project-manager', 'rustic-editor', 'rustic-runtime', 'rustic-asset-worker', 'rustic-agent-backend', 'rustic-luau-host', 'rustic-setup-helper')
    foreach ($process in @(Get-Process -Name $names -ErrorAction SilentlyContinue)) {
        try {
            $path = $process.Path
            # An inaccessible process could belong to this installation. Wait safely.
            if (-not $path -or (Split-Path -Parent $path) -eq $Directory) { return $true }
        } catch { return $true }
    }
    return $false
}

function Wait-DesktopExit([string]$Directory, [string]$CacheDirectory, [string]$Tag) {
    $requestFile = Join-Path $CacheDirectory 'install-request.txt'
    $lastRequest = ''
    $requestWait = 0
    $ticks = 0
    while (Test-RusticRunning $Directory) {
        $request = if (Test-Path -LiteralPath $requestFile) {
            [IO.File]::ReadAllText($requestFile)
        } else { '' }
        # A dismissed update must not pin the worker to that release forever.
        # Finish any graceful-close request before looking for a newer release.
        if ($ticks -ge 3600 -and -not $request) { return $false }
        if ($request -cne $lastRequest) { $requestWait = 0; $lastRequest = $request }
        if ($request) {
            $requestWait++
            if ($requestWait -ge 120) {
                # A runtime/worker, inaccessible process or newly opened editor may
                # still be running. Give closed windows back instead of trapping
                # the user outside the editor while waiting indefinitely.
                Remove-Item -LiteralPath $requestFile -Force -ErrorAction SilentlyContinue
                Write-UpdateLog 'Update deferred after waiting two minutes for Rustic processes. Restoring closed windows; the Update button remains available.'
                Restart-DesktopSessions $Directory $CacheDirectory $Tag
                $lastRequest = ''
                $requestWait = 0
            }
        }
        if (($ticks % 60) -eq 0) {
            $blockers = @(Get-Process -Name 'rustic-*' -ErrorAction SilentlyContinue | ForEach-Object {
                try {
                    if (-not $_.Path -or (Split-Path -Parent $_.Path) -eq $Directory) {
                        "$($_.ProcessName) (PID $($_.Id))"
                    }
                } catch { "$($_.ProcessName) (PID $($_.Id), path unavailable)" }
            })
            Write-UpdateLog "Waiting for Rustic to close: $($blockers -join ', '). Close standalone games/workers separately."
        }
        Start-Sleep -Seconds 1
        $ticks++
    }
    return $true
}

function Install-DesktopUpdate([string]$Installer, [string]$Directory, [string]$LogDirectory) {
    $scope = (Get-Content -LiteralPath (Join-Path $Directory 'install-scope.txt') -Raw).Trim()
    if ($scope -notin @('allusers', 'currentuser')) { throw 'Unknown installation scope.' }
    # Preserve Setup's previous component/task selections and the installation directory.
    # Never close an editor or reboot the computer from the updater.
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOCLOSEAPPLICATIONS',
        '/NORESTARTAPPLICATIONS', "/$scope", ('/DIR="' + $Directory + '"'),
        ('/LOG="' + (Join-Path $LogDirectory 'update-setup.log') + '"'))
    $process = Start-Process -FilePath $Installer -ArgumentList $arguments -PassThru -Wait
    if ($process.ExitCode -ne 0) { throw "Update installer exited with code $($process.ExitCode)." }
}

function Write-UpdateLog([string]$Message) {
    Add-Content -LiteralPath (Join-Path $script:CacheDirectory 'update.log') `
        -Value (('{0:u} ' -f [DateTime]::UtcNow) + $Message)
}

function Restart-DesktopSessions([string]$Directory, [string]$CacheDirectory, [string]$Tag) {
    foreach ($record in @(Get-ChildItem -LiteralPath $CacheDirectory -Filter 'resume-*.txt' -File)) {
        try {
            $lines = @(Get-Content -LiteralPath $record.FullName -Encoding UTF8)
            if ($lines.Count -lt 2 -or $lines[0] -cne $Tag) { continue }
            $start = @{ WorkingDirectory = $Directory }
            if ($lines[1] -eq 'project-manager') {
                $start.FilePath = Join-Path $Directory 'rustic-project-manager.exe'
            } elseif ($lines[1] -eq 'editor') {
                if ($lines.Count -lt 3 -or -not [IO.Path]::IsPathRooted($lines[2]) -or
                    -not (Test-Path -LiteralPath $lines[2] -PathType Container)) {
                    throw 'The project to restore no longer exists.'
                }
                $start.FilePath = Join-Path $Directory 'rustic-editor.exe'
                $quotedProject = $lines[2] -replace '(\\+)$', '$1$1'
                $start.ArgumentList = '--project "' + $quotedProject + '"'
            } else { throw 'Unknown desktop session type.' }
            # This worker retains the original user's context even when Setup elevated.
            Start-Process @start | Out-Null
            Write-UpdateLog "Restored $($lines[1]) for update $Tag."
        } catch { Write-UpdateLog "Could not restore a desktop session: $($_.Exception.Message)" }
        finally { Remove-Item -LiteralPath $record.FullName -Force -ErrorAction SilentlyContinue }
    }
}

function Invoke-DesktopUpdateCheck([string]$Directory, [string]$CacheDirectory) {
    $commitFile = Join-Path $Directory 'build-commit.txt'
    $commit = (Get-Content -LiteralPath $commitFile -Raw).Trim()
    $asset = Select-DesktopUpdate @(Get-PublishedReleases) $commit
    if (-not $asset) { return $null }
    Write-UpdateLog "Preparing $($asset.name)."
    Remove-Item -LiteralPath (Join-Path $CacheDirectory 'pending-update.txt') -Force -ErrorAction SilentlyContinue
    $installer = Save-VerifiedInstaller $asset $CacheDirectory
    $tag = ($asset.browser_download_url -split '/')[-2]
    # Publish only after verification. ASCII fields also decode identically in Rust.
    $pending = Join-Path $CacheDirectory 'pending-update.txt'
    $temporary = $pending + '.tmp'
    [IO.File]::WriteAllText($temporary, "$commit`n$tag`n", [Text.Encoding]::ASCII)
    Move-Item -LiteralPath $temporary -Destination $pending -Force
    Write-UpdateLog 'Installer verified and ready. Waiting for Rustic to close.'
    if (-not (Wait-DesktopExit $Directory $CacheDirectory $tag)) {
        return [pscustomobject]@{ Installed = $false; Refresh = $true; Tag = $tag }
    }
    if (-not (Test-Path -LiteralPath $commitFile)) { return [pscustomobject]@{ Installed = $false; Tag = $tag } }
    if ((Get-Content -LiteralPath $commitFile -Raw).Trim() -ne $commit) { return [pscustomobject]@{ Installed = $false; Tag = $tag } }
    Write-UpdateLog 'Starting Setup. See update-setup.log for installation progress.'
    try { Install-DesktopUpdate $installer $Directory $CacheDirectory }
    catch {
        Remove-Item -LiteralPath (Join-Path $CacheDirectory 'install-request.txt') -Force -ErrorAction SilentlyContinue
        Get-ChildItem -LiteralPath $CacheDirectory -Filter 'resume-*.txt' -File | Remove-Item -Force
        throw
    }
    Write-UpdateLog 'Update installed.'
    Remove-Item -LiteralPath $installer -Force
    Remove-Item -LiteralPath $pending -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $CacheDirectory 'install-request.txt') -Force -ErrorAction SilentlyContinue
    return [pscustomobject]@{ Installed = $true; Tag = $tag }
}

function Invoke-DesktopUpdater([string]$Directory) {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { return }
    $Directory = (Resolve-Path -LiteralPath $Directory).Path.TrimEnd('\')
    $commitFile = Join-Path $Directory 'build-commit.txt'
    if (-not (Test-Path -LiteralPath $commitFile)) { return }
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { $key = [BitConverter]::ToString($hasher.ComputeHash([Text.Encoding]::UTF8.GetBytes($Directory.ToLowerInvariant()))).Replace('-', '') }
    finally { $hasher.Dispose() }
    $script:CacheDirectory = if ($StateDirectory) { $StateDirectory } else {
        Join-Path $env:LOCALAPPDATA "Rustic Game Engine\updates\$key"
    }
    New-Item -ItemType Directory -Force -Path $script:CacheDirectory | Out-Null
    # FileShare.None permits only one worker per installation, across launcher/editor instances.
    try { $lock = [IO.File]::Open((Join-Path $script:CacheDirectory 'updater.lock'), 'OpenOrCreate', 'ReadWrite', 'None') }
    catch [IO.IOException] { return }
    try {
        do {
            try {
                $result = Invoke-DesktopUpdateCheck $Directory $script:CacheDirectory
                if ($result) {
                    if ($result.PSObject.Properties['Refresh'] -and $result.Refresh) { continue }
                    # Release the worker lock before reopening apps: their new updater must
                    # be able to acquire it and keep checking during the restored session.
                    $lock.Dispose()
                    $lock = $null
                    if ($result.Installed) {
                        try { Restart-DesktopSessions $Directory $script:CacheDirectory $result.Tag }
                        catch { Write-UpdateLog "Could not restore desktop sessions: $($_.Exception.Message)" }
                    }
                    return
                }
            } catch { Write-UpdateLog $_.Exception.Message }
            # Check hourly while Rustic is open; a later launch retries network/Setup failures.
            for ($tick = 0; $tick -lt 720; $tick++) {
                if (-not (Test-RusticRunning $Directory)) { return }
                Start-Sleep -Seconds 5
            }
        } while ($true)
    } finally { if ($lock) { $lock.Dispose() } }
}

if (-not $FunctionsOnly) { Invoke-DesktopUpdater $InstallDirectory }
