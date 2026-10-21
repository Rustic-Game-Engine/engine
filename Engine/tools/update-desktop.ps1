[CmdletBinding()]
param(
    [string]$InstallDirectory = (Split-Path -Parent $PSScriptRoot),
    [switch]$FunctionsOnly
)

$ErrorActionPreference = 'Stop'
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
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $Asset.browser_download_url -Headers $script:Headers `
            -OutFile $partial -TimeoutSec 600 | Out-Null
        if ((Get-Item -LiteralPath $partial).Length -ne $Asset.size) { throw 'Installer size does not match GitHub.' }
        $expected = $Asset.digest.Substring(7)
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

function Invoke-DesktopUpdateCheck([string]$Directory, [string]$CacheDirectory) {
    $commitFile = Join-Path $Directory 'build-commit.txt'
    $commit = (Get-Content -LiteralPath $commitFile -Raw).Trim()
    $asset = Select-DesktopUpdate @(Get-PublishedReleases) $commit
    if (-not $asset) { return $false }
    Write-UpdateLog "Downloading $($asset.name). Close Rustic to install the update."
    $installer = Save-VerifiedInstaller $asset $CacheDirectory
    while (Test-RusticRunning $Directory) { Start-Sleep -Seconds 5 }
    if (-not (Test-Path -LiteralPath $commitFile)) { return $true }
    if ((Get-Content -LiteralPath $commitFile -Raw).Trim() -ne $commit) { return $true }
    Install-DesktopUpdate $installer $Directory $CacheDirectory
    Write-UpdateLog 'Update installed. Start Rustic normally to use the new release.'
    Remove-Item -LiteralPath $installer -Force
    return $true
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
    $script:CacheDirectory = Join-Path $env:LOCALAPPDATA "Rustic Game Engine\updates\$key"
    New-Item -ItemType Directory -Force -Path $script:CacheDirectory | Out-Null
    # FileShare.None permits only one worker per installation, across launcher/editor instances.
    try { $lock = [IO.File]::Open((Join-Path $script:CacheDirectory 'updater.lock'), 'OpenOrCreate', 'ReadWrite', 'None') }
    catch [IO.IOException] { return }
    try {
        do {
            try {
                if (Invoke-DesktopUpdateCheck $Directory $script:CacheDirectory) { return }
            } catch { Write-UpdateLog $_.Exception.Message }
            # Check hourly while Rustic is open; a later launch retries network/Setup failures.
            for ($tick = 0; $tick -lt 720; $tick++) {
                if (-not (Test-RusticRunning $Directory)) { return }
                Start-Sleep -Seconds 5
            }
        } while ($true)
    } finally { $lock.Dispose() }
}

if (-not $FunctionsOnly) { Invoke-DesktopUpdater $InstallDirectory }
