[CmdletBinding()]
param(
    [string]$Languages,
    [string]$LogPath,
    [switch]$CheckOnly,
    [switch]$Setup
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

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
    Write-Host 'Lua 5.4, Luau, JavaScript/QuickJS, and HTML/CSS support are already bundled.'
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

function Assert-WinGetInitialized([int]$ExitCode) {
    # Stop the whole batch if the host cannot initialize, instead of launching
    # the same broken executable again for each selected language.
    if ($ExitCode -in @(142, -1073741502, 3221225794)) {
        throw "Windows Package Manager failed to initialize (exit code $ExitCode). Repair Microsoft App Installer before retrying."
    }
}

function Invoke-WinGet([string]$Executable, [string[]]$Arguments) {
    # Native '& winget' can make PowerShell allocate a hidden console even when
    # Setup started PowerShell with CREATE_NO_WINDOW. Keep WinGet console-free too.
    $info = [System.Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable
    # Callers supply only fixed flags and catalog package IDs, with no whitespace.
    $info.Arguments = $Arguments -join ' '
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::Start($info)
    try {
        # Drain both pipes concurrently so a full stderr pipe cannot deadlock stdout.
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $output = $stdout.GetAwaiter().GetResult()
        $errors = $stderr.GetAwaiter().GetResult()
        if ($output) { Write-Host $output.TrimEnd() }
        if ($errors) { Write-Host $errors.TrimEnd() }
        return $process.ExitCode
    }
    finally { $process.Dispose() }
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
    $code = Invoke-WinGet $wingetPath @('list', '--id', $Entry.Package, '--exact', '--source', 'winget',
        '--disable-interactivity', '--accept-source-agreements')
    Assert-WinGetInitialized $code
    return $code -eq 0
}

$exitCode = 0
$transcriptStarted = $false
try {
    if ($Setup -and [string]::IsNullOrWhiteSpace($LogPath)) {
        # Resolve this in the original user's process, not elevated Setup's profile.
        $LogPath = Join-Path $env:LOCALAPPDATA 'Rustic Game Engine\logs\toolchain-install.log'
    }
    if (-not [string]::IsNullOrWhiteSpace($LogPath)) {
        $logDirectory = Split-Path -Parent $LogPath
        if ($logDirectory) {
            New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
        }
        Start-Transcript -Path $LogPath -Append | Out-Null
        $transcriptStarted = $true
    }

    if ($Setup -and [string]::IsNullOrWhiteSpace($Languages)) {
        throw 'Setup must supply the selected languages; interactive selection is unavailable.'
    }
    $selectedLanguages = if ([string]::IsNullOrWhiteSpace($Languages)) {
        @(Read-LanguageSelection)
    }
    else {
        @($Languages.Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    }
    $selectedLanguages = @($selectedLanguages | Select-Object -Unique)

    foreach ($language in $selectedLanguages) {
        if (-not $catalog.Contains($language)) { throw "Unknown language selection '$language'." }
    }

    if ($CheckOnly) {
        foreach ($language in $selectedLanguages) {
            if (-not (Test-ToolchainInstalled $catalog[$language])) { $exitCode = 1 }
        }
    }
    else {
        $winget = $null
        $failures = [System.Collections.Generic.List[string]]::new()
        foreach ($language in $selectedLanguages) {
            $entry = $catalog[$language]
            Write-Host "Checking $($entry.Name)..."
            if (Test-ToolchainInstalled $entry) {
                Write-Host "$($entry.Name) is already installed."
                continue
            }

            if (-not $winget) { $winget = Find-WinGet }
            Write-Host "Installing $($entry.Name)..."
            $packageExitCode = Invoke-WinGet $winget @('install', '--id', $entry.Package, '--exact',
                '--source', 'winget', '--silent', '--disable-interactivity',
                '--accept-source-agreements', '--accept-package-agreements')
            Write-Host "WinGet exit code: $packageExitCode"
            Assert-WinGetInitialized $packageExitCode
            if ($packageExitCode -ne 0) {
                $failure = "$($entry.Name) (WinGet exit code $packageExitCode)"
                Write-Host "Failed: $failure"
                $failures.Add($failure)
            }
        }

        if ($failures.Count -gt 0) {
            throw ("The following toolchains could not be installed: " + ($failures -join ', ') + '.')
        }

        Write-Host 'Selected gameplay language toolchains are installed.'
        Write-Host 'Restart Rustic Game Engine if it was already open so it can refresh PATH.'
    }
}
catch {
    Write-Error $_ -ErrorAction Continue
    $exitCode = 1
}
finally {
    if ($transcriptStarted) { Stop-Transcript | Out-Null }
}
exit $exitCode
