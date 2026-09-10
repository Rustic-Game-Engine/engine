[CmdletBinding()]
param(
    [string]$Languages,
    [string]$LogPath,
    [switch]$CheckOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not [string]::IsNullOrWhiteSpace($LogPath)) {
    $logDirectory = Split-Path -Parent $LogPath
    if ($logDirectory) {
        New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
    }
    Start-Transcript -Path $LogPath -Append | Out-Null
}

$catalog = [ordered]@{
    Python = [pscustomobject]@{ Name = 'Python 3.14'; Package = 'Python.Python.3.14'; Commands = @('python.exe', 'py.exe') }
    CSharp = [pscustomobject]@{ Name = '.NET SDK 10 for C#'; Package = 'Microsoft.DotNet.SDK.10'; Commands = @('dotnet.exe') }
    CCpp = [pscustomobject]@{ Name = 'LLVM for C and C++'; Package = 'LLVM.LLVM'; Commands = @('clang.exe', 'clang-cl.exe') }
    Java = [pscustomobject]@{ Name = 'Microsoft OpenJDK 21'; Package = 'Microsoft.OpenJDK.21'; Commands = @('javac.exe') }
    Php = [pscustomobject]@{ Name = 'PHP 8.4 CLI'; Package = 'PHP.PHP.8.4'; Commands = @('php.exe') }
}

function Find-WinGet {
    $command = Get-Command winget.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }

    $alias = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\winget.exe'
    if (Test-Path -LiteralPath $alias -PathType Leaf) { return $alias }

    throw 'Windows Package Manager is unavailable. Install or repair Microsoft App Installer, then run Language Toolchain Manager again.'
}

function Read-LanguageSelection {
    Write-Host ''
    Write-Host 'Rustic Game Engine - Gameplay Language Toolchains'
    Write-Host 'Lua 5.4, JavaScript/QuickJS, and HTML/CSS support are already bundled.'
    Write-Host 'Select any additional toolchains to install:'
    $keys = @($catalog.Keys)
    for ($index = 0; $index -lt $keys.Count; $index++) {
        Write-Host ("  {0}. {1}" -f ($index + 1), $catalog[$keys[$index]].Name)
    }
    Write-Host 'Enter numbers separated by commas, or press Enter to cancel.'
    $answer = Read-Host 'Selection'
    if ([string]::IsNullOrWhiteSpace($answer)) { return @() }

    $selected = foreach ($part in $answer.Split(',')) {
        $number = 0
        if (-not [int]::TryParse($part.Trim(), [ref]$number) -or $number -lt 1 -or $number -gt $keys.Count) {
            throw "Invalid selection '$part'."
        }
        $keys[$number - 1]
    }
    return @($selected | Select-Object -Unique)
}

function Test-ToolchainInstalled($Entry) {
    foreach ($commandName in $Entry.Commands) {
        if (Get-Command $commandName -ErrorAction SilentlyContinue) { return $true }
    }

    $wingetCommand = Get-Command winget.exe -ErrorAction SilentlyContinue
    if (-not $wingetCommand) {
        $alias = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\winget.exe'
        if (Test-Path -LiteralPath $alias -PathType Leaf) { $wingetCommand = $alias }
    }
    if (-not $wingetCommand) { return $false }

    $wingetPath = if ($wingetCommand -is [string]) { $wingetCommand } else { $wingetCommand.Source }
    & $wingetPath list --id $Entry.Package --exact --source winget `
        --disable-interactivity --accept-source-agreements *> $null
    return $LASTEXITCODE -eq 0
}

$selectedLanguages = if ([string]::IsNullOrWhiteSpace($Languages)) {
    @(Read-LanguageSelection)
}
else {
    @($Languages.Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ })
}
$selectedLanguages = @($selectedLanguages)

if ($selectedLanguages.Count -eq 0) {
    Write-Host 'No external language toolchains selected.'
    exit 0
}

foreach ($language in $selectedLanguages) {
    if (-not $catalog.Contains($language)) { throw "Unknown language selection '$language'." }
}

if ($CheckOnly) {
    foreach ($language in $selectedLanguages) {
        if (-not (Test-ToolchainInstalled $catalog[$language])) { exit 1 }
    }
    exit 0
}

$winget = Find-WinGet
$failures = [System.Collections.Generic.List[string]]::new()
foreach ($language in $selectedLanguages) {
    $entry = $catalog[$language]
    if (Test-ToolchainInstalled $entry) {
        Write-Host "$($entry.Name) is already installed."
        continue
    }

    Write-Host "Installing $($entry.Name)..."
    & $winget install --id $entry.Package --exact --source winget --silent `
        --disable-interactivity --accept-source-agreements --accept-package-agreements
    if ($LASTEXITCODE -ne 0) {
        $failures.Add("$($entry.Name) (WinGet exit code $LASTEXITCODE)")
    }
}

if ($failures.Count -gt 0) {
    Write-Error ("The following toolchains could not be installed: " + ($failures -join ', ') + '.')
    exit 1
}

Write-Host 'Selected gameplay language toolchains are installed.'
Write-Host 'Restart Rustic Game Engine if it was already open so it can refresh PATH.'
