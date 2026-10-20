[CmdletBinding()]
param([string]$InstallerPath)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Run with Windows PowerShell 5.1, the host used by the shipped installer.
if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw 'These tests require Windows.'
}

$manager = Join-Path $PSScriptRoot 'manage-language-toolchains.ps1'
$powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('rustic-toolchains-' + [guid]::NewGuid())
$mockBin = Join-Path $testRoot 'bin'
New-Item -ItemType Directory -Path $mockBin -Force | Out-Null

# A real native executable exercises native exit codes, including signed NTSTATUS.
# It never installs software or accesses the network.
Add-Type -OutputAssembly (Join-Path $mockBin 'winget.exe') -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
public class MockWinGet {
    [DllImport("kernel32.dll")] static extern IntPtr GetConsoleWindow();
    public static int Main(string[] args) {
        if (GetConsoleWindow() != IntPtr.Zero) return 92;
        File.AppendAllText(Environment.GetEnvironmentVariable("RUSTIC_TEST_CALLS"),
            string.Join(" ", args) + Environment.NewLine);
        string mode = Environment.GetEnvironmentVariable("RUSTIC_TEST_MODE");
        Console.WriteLine("Mock WinGet: " + string.Join(" ", args));
        if (args[0] == "list") {
            if (mode == "fatal-list") return unchecked((int)0xC0000142);
            if (mode == "fatal-list-142") return 142;
            return mode == "installed" ? 0 : 1;
        }
        if (mode == "fatal-install") return unchecked((int)0xC0000142);
        if (mode == "package-failure" && Array.IndexOf(args, "PHP.PHP.8.4") >= 0) return 9;
        return 0;
    }
}
'@

function Assert-Equal($Actual, $Expected, [string]$Message) {
    if ($Actual -ne $Expected) { throw "$Message (expected $Expected, got $Actual)" }
}

function New-TestProcess([string]$Mode) {
    $caseRoot = Join-Path $testRoot ($Mode + '-' + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $caseRoot | Out-Null
    $info = [System.Diagnostics.ProcessStartInfo]::new()
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    # Isolate discovery from the runner's installed toolchains and App Installer.
    $info.EnvironmentVariables['PATH'] = $mockBin
    $info.EnvironmentVariables['LOCALAPPDATA'] = $caseRoot
    $info.EnvironmentVariables['USERPROFILE'] = $caseRoot
    $info.EnvironmentVariables['RUSTIC_TEST_MODE'] = $Mode
    $info.EnvironmentVariables['RUSTIC_TEST_CALLS'] = Join-Path $caseRoot 'calls.txt'
    return $info
}

function Get-Calls($Info) {
    $path = $Info.EnvironmentVariables['RUSTIC_TEST_CALLS']
    if (Test-Path -LiteralPath $path) { Get-Content -LiteralPath $path }
}

function Invoke-ManagerCase([string]$Mode, [string]$Languages, [int]$ExpectedCode,
    [int]$ExpectedCalls, [string]$ExpectedLog, [switch]$CheckOnly) {
    $info = New-TestProcess $Mode
    $info.FileName = $powershell
    $info.Arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' +
        $manager + '" -Languages "' + $Languages + '"'
    if ($CheckOnly) { $info.Arguments += ' -CheckOnly' } else { $info.Arguments += ' -Setup' }
    $process = [System.Diagnostics.Process]::Start($info)
    try {
        if (-not $process.WaitForExit(60000)) { $process.Kill(); throw "$Mode timed out" }
        Assert-Equal $process.ExitCode $ExpectedCode "$Mode exit code"
    }
    finally { $process.Dispose() }
    Assert-Equal @(Get-Calls $info).Count $ExpectedCalls "$Mode WinGet invocation count"
    $log = Join-Path $info.EnvironmentVariables['LOCALAPPDATA'] 'Rustic Game Engine\logs\toolchain-install.log'
    if ($CheckOnly) {
        Assert-Equal (Test-Path -LiteralPath $log) $false 'CheckOnly must not create a transcript'
    }
    else {
        $text = Get-Content -LiteralPath $log -Raw
        if (-not $text.Contains($ExpectedLog)) {
            Write-Host $text
            throw "$Mode transcript did not contain '$ExpectedLog'"
        }
    }
    Write-Host "PASS: $Mode ($Languages)"
}

function Invoke-InstallerCase([string]$Mode, [string]$Components, [int]$ExpectedCalls,
    [bool]$ExpectFailure) {
    $info = New-TestProcess $Mode
    $info.FileName = (Resolve-Path -LiteralPath $InstallerPath).Path
    $caseRoot = $info.EnvironmentVariables['LOCALAPPDATA']
    $installRoot = Join-Path $caseRoot 'app'
    $setupLog = Join-Path $caseRoot 'setup.log'
    $info.Arguments = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CURRENTUSER /DIR="' +
        $installRoot + '" /LOG="' + $setupLog + '" /COMPONENTS="' + $Components + '"'
    $process = [System.Diagnostics.Process]::Start($info)
    try {
        if (-not $process.WaitForExit(120000)) { $process.Kill(); throw "$Mode Setup timed out" }
        Assert-Equal $process.ExitCode 0 "$Mode Setup must finish installing the engine"
    }
    finally { $process.Dispose() }
    Assert-Equal (Test-Path (Join-Path $installRoot 'rustic-project-manager.exe')) $true 'Engine installed'
    Assert-Equal (Test-Path (Join-Path $installRoot 'rustic-setup-helper.exe')) $true 'Setup helper installed'
    Assert-Equal @(Get-Calls $info).Count $ExpectedCalls "$Mode Setup WinGet invocation count"
    $log = Get-Content -LiteralPath $setupLog -Raw
    Assert-Equal ($log.Contains('Agent integration installation exited with code')) $false 'Agent integrations must initialize successfully'
    Assert-Equal ($log.Contains('Could not start the integration installer')) $false 'Integration helper must start'
    Assert-Equal ($log.Contains('Language Toolchain Manager exited with code')) $ExpectFailure 'Setup failure diagnostic'
    $transcript = Join-Path $caseRoot 'Rustic Game Engine\logs\toolchain-install.log'
    if ($Components -eq 'engine,languages\bundled') {
        Assert-Equal (Test-Path $transcript) $false 'Bundled-only Setup must not launch PowerShell'
    }
    else {
        # One transcript header means all selections shared one PowerShell process.
        $text = Get-Content -LiteralPath $transcript -Raw
        Assert-Equal ([regex]::Matches($text, 'Windows PowerShell transcript start').Count) 1 'Setup PowerShell launch count'
    }
    $launchLog = Get-Content -LiteralPath (Join-Path $caseRoot 'Rustic Game Engine\logs\setup-launch.log') -Raw
    if (-not $launchLog.Contains('--install-user-integrations exited with code 0')) {
        throw 'Packaged agent integration launch did not succeed'
    }
    # Exercise the installed GUI-subsystem executable without requiring a GPU.
    $smokeInfo = New-TestProcess 'project-manager-smoke'
    $smokeInfo.FileName = Join-Path $installRoot 'rustic-project-manager.exe'
    $smokeInfo.Arguments = '--headless-smoke'
    $smoke = [System.Diagnostics.Process]::Start($smokeInfo)
    try {
        if (-not $smoke.WaitForExit(60000)) { $smoke.Kill(); throw 'Installed project manager timed out' }
        Assert-Equal $smoke.ExitCode 0 'Installed project manager startup'
    }
    finally { $smoke.Dispose() }
    Write-Host "PASS: packaged Setup $Mode"
}

try {
    $all = 'Python,CSharp,CCpp,Java,Php'
    Invoke-ManagerCase 'installed' $all 0 5 'already installed'
    Invoke-ManagerCase 'missing' $all 0 10 'Selected gameplay language toolchains are installed.'
    Invoke-ManagerCase 'fatal-list' $all 1 1 'failed to initialize (exit code -1073741502)'
    Invoke-ManagerCase 'fatal-list-142' $all 1 1 'failed to initialize (exit code 142)'
    Invoke-ManagerCase 'fatal-install' $all 1 2 'failed to initialize (exit code -1073741502)'
    Invoke-ManagerCase 'package-failure' 'Php,Python' 1 4 'PHP 8.4 CLI (WinGet exit code 9)'
    Invoke-ManagerCase 'missing' 'Python,Python' 0 2 'Selected gameplay language toolchains are installed.'
    Invoke-ManagerCase 'installed' 'Python' 0 1 '' -CheckOnly
    Invoke-ManagerCase 'missing' 'Python' 1 1 '' -CheckOnly
    Invoke-ManagerCase 'missing' 'Unknown' 1 0 "Unknown language selection 'Unknown'"

    # An already-installed toolchain does not require WinGet to be present.
    Copy-Item (Join-Path $mockBin 'winget.exe') (Join-Path $mockBin 'python.exe')
    Rename-Item (Join-Path $mockBin 'winget.exe') 'winget-disabled.exe'
    Invoke-ManagerCase 'installed' 'Python' 0 0 'already installed'
    Remove-Item (Join-Path $mockBin 'python.exe')
    Invoke-ManagerCase 'missing' 'Python' 1 0 'Windows Package Manager is unavailable.'
    Rename-Item (Join-Path $mockBin 'winget-disabled.exe') 'winget.exe'

    if ($InstallerPath) {
        $components = 'engine,languages\bundled,languages\python,languages\csharp,languages\ccpp,languages\java,languages\php'
        Invoke-InstallerCase 'missing' $components 10 $false
        Invoke-InstallerCase 'fatal-list' $components 1 $true
        Invoke-InstallerCase 'fatal-install' $components 2 $true
        Invoke-InstallerCase 'missing' 'engine,languages\bundled' 0 $false
    }
}
finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force
}
