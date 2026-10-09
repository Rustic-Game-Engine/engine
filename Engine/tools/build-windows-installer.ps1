[CmdletBinding()]
param(
    [switch]$InstallDirectoryOnPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw 'The Windows installer must be built on Windows.'
}

$workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$toolRoot = Join-Path $workspaceRoot '.tools'
$localCargoHome = Join-Path $toolRoot 'cargo'
$localRustupHome = Join-Path $toolRoot 'rustup'
$cargo = Join-Path $localCargoHome 'bin\cargo.exe'
$rustupInit = Join-Path $toolRoot 'rustup-init.exe'
$innoRoot = Join-Path $toolRoot 'inno-setup'
$iscc = Join-Path $innoRoot 'ISCC.exe'

New-Item -ItemType Directory -Force -Path $toolRoot | Out-Null

if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    Write-Host 'Installing the pinned Rust toolchain locally under .tools...'
    $rustupUri = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
    if (-not (Test-Path -LiteralPath $rustupInit -PathType Leaf)) {
        Invoke-WebRequest -UseBasicParsing -Uri $rustupUri -OutFile $rustupInit
    }
    # Official rustup executables are unsigned; verify the published checksum instead.
    $checksumResponse = Invoke-WebRequest -UseBasicParsing -Uri "$rustupUri.sha256"
    $checksumText = if ($checksumResponse.Content -is [byte[]]) {
        [System.Text.Encoding]::UTF8.GetString($checksumResponse.Content)
    } else {
        [string]$checksumResponse.Content
    }
    $expectedChecksum = ($checksumText.Trim() -split '\s+')[0]
    if ($expectedChecksum -notmatch '^[0-9a-fA-F]{64}$') {
        throw 'The published rustup SHA-256 checksum is invalid.'
    }
    $actualChecksum = (Get-FileHash -LiteralPath $rustupInit -Algorithm SHA256).Hash
    if ($actualChecksum -ne $expectedChecksum) {
        Remove-Item -LiteralPath $rustupInit -Force
        throw 'The downloaded rustup installer does not match its published SHA-256 checksum.'
    }
    $env:CARGO_HOME = $localCargoHome
    $env:RUSTUP_HOME = $localRustupHome
    & $rustupInit -y --no-modify-path --profile minimal
    if ($LASTEXITCODE -ne 0) { throw "rustup-init failed with exit code $LASTEXITCODE." }
}

$env:CARGO_HOME = $localCargoHome
$env:RUSTUP_HOME = $localRustupHome
$env:Path = "$(Join-Path $localCargoHome 'bin');$env:Path"

if (-not (Test-Path -LiteralPath $iscc -PathType Leaf)) {
    Write-Host 'Installing Inno Setup locally under .tools...'
    $innoInstaller = Join-Path $toolRoot 'innosetup-installer.exe'
    Invoke-WebRequest -UseBasicParsing `
        -Uri 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' `
        -OutFile $innoInstaller
    $innoSignature = Get-AuthenticodeSignature -LiteralPath $innoInstaller
    if ($innoSignature.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
        throw "The downloaded Inno Setup installer has an invalid signature: $($innoSignature.Status)."
    }
    $innoInstallProcess = Start-Process -FilePath $innoInstaller `
        -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', "/DIR=$innoRoot") `
        -WindowStyle Hidden -Wait -PassThru
    if ($innoInstallProcess.ExitCode -ne 0) {
        throw "Inno Setup installation failed with exit code $($innoInstallProcess.ExitCode)."
    }
}

Push-Location $workspaceRoot
try {
    Write-Host 'Building bundled Luau API host...'
    & $cargo build --locked --manifest-path 'apps/luau-host/Cargo.toml' --profile distribution --target-dir 'target'
    if ($LASTEXITCODE -ne 0) { throw "Luau host build failed with exit code $LASTEXITCODE." }
    Write-Host 'Building Rustic Game Engine distribution binaries...'
    & $cargo build --locked --profile distribution `
        --package rustic-project-manager `
        --package rustic-editor `
        --package rustic-runtime `
        --package rustic-asset-worker `
        --package rustic-agent-backend
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed with exit code $LASTEXITCODE." }

    $metadata = (& $cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw "Cargo metadata failed with exit code $LASTEXITCODE." }
    $appVersion = ($metadata.packages | Where-Object name -eq 'rustic-project-manager').version
    if (-not $appVersion) { throw 'Could not determine the application version from Cargo metadata.' }

    $pathFlag = if ($InstallDirectoryOnPath) { '1' } else { '0' }
    $buildSuffix = '-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss')
    Write-Host 'Creating the Windows installer...'
    & $iscc "/DAppVersion=$appVersion" "/DBuildSuffix=$buildSuffix" "/DAddToPathByDefault=$pathFlag" 'installer\RusticGameEngine.iss'
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE." }
}
finally {
    Pop-Location
}

$installer = Get-ChildItem -LiteralPath (Join-Path $workspaceRoot 'dist') `
    -Filter 'RusticGameEngine-Setup-*.exe' |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1

if (-not $installer) { throw 'The installer compiler succeeded but no installer was found.' }
Write-Host "Installer created: $($installer.FullName)"
