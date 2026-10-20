[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$HelperPath,
    [Parameter(Mandatory = $true)][string]$ProjectManagerPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-Equal($Actual, $Expected, [string]$Message) {
    if ($Actual -ne $Expected) { throw "$Message (expected $Expected, got $Actual)" }
}

function Assert-GuiSubsystem([string]$Path) {
    $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path).Path)
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    Assert-Equal ([BitConverter]::ToUInt16($bytes, $peOffset + 24 + 68)) 2 "$Path must use the Windows GUI subsystem"
}

Assert-GuiSubsystem $HelperPath
Assert-GuiSubsystem $ProjectManagerPath

# Spaces in the path exercise native argument quoting. The fixture fails if
# Windows allocated a console, even a hidden one, for the background command.
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('rustic launch ' + [guid]::NewGuid())
New-Item -ItemType Directory -Path (Join-Path $testRoot 'tools') -Force | Out-Null
try {
    $helper = Join-Path $testRoot 'rustic-setup-helper.exe'
    Copy-Item -LiteralPath $HelperPath -Destination $helper
    Add-Type -OutputAssembly (Join-Path $testRoot 'rustic-agent-backend.exe') -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class MockBackend {
    [DllImport("kernel32.dll")] static extern IntPtr GetConsoleWindow();
    public static int Main(string[] args) {
        if (GetConsoleWindow() != IntPtr.Zero) return 90;
        if (args.Length != 1 || args[0] != "--install-user-integrations") return 91;
        Console.WriteLine("Backend stdout captured");
        Console.Error.WriteLine("Backend stderr captured");
        return int.Parse(Environment.GetEnvironmentVariable("RUSTIC_TEST_EXIT_CODE"));
    }
}
'@
    Set-Content -LiteralPath (Join-Path $testRoot 'tools\manage-language-toolchains.ps1') -Value @'
param([string]$Languages, [switch]$Setup)
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class ConsoleProbe {
    [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
}
"@
if ([ConsoleProbe]::GetConsoleWindow() -ne [IntPtr]::Zero) { exit 90 }
if (-not $Setup -or $Languages -ne 'Python,CSharp,CCpp,Java,Php') { exit 91 }
Write-Output 'PowerShell stdout captured'
[Console]::Error.WriteLine('PowerShell stderr captured')
exit ([int]$env:RUSTIC_TEST_EXIT_CODE)
'@

    function Invoke-HelperCase([string]$Arguments, [int]$ChildCode, [int]$ExpectedCode, [string]$ExpectedLog) {
        $info = [System.Diagnostics.ProcessStartInfo]::new()
        $info.FileName = $helper
        $info.Arguments = $Arguments
        $info.UseShellExecute = $false
        # Do not mask console allocation in the helper itself; its PE subsystem
        # must prevent it, and CREATE_NO_WINDOW must prevent it in each child.
        $info.CreateNoWindow = $false
        $caseRoot = Join-Path $testRoot ([guid]::NewGuid().ToString())
        $info.EnvironmentVariables['LOCALAPPDATA'] = $caseRoot
        $info.EnvironmentVariables['RUSTIC_TEST_EXIT_CODE'] = [string]$ChildCode
        $process = [System.Diagnostics.Process]::Start($info)
        try {
            if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Setup helper timed out' }
            Assert-Equal $process.ExitCode $ExpectedCode "$Arguments exit code"
        }
        finally { $process.Dispose() }
        $log = Get-Content -LiteralPath (Join-Path $caseRoot 'Rustic Game Engine\logs\setup-launch.log') -Raw
        if (-not $log.Contains($ExpectedLog)) { throw "Missing helper log output: $ExpectedLog" }
        return $log
    }

    foreach ($code in @(0, 9, -1073741502)) {
        $log = Invoke-HelperCase '--install-user-integrations' $code $code 'Backend stdout captured'
        if (-not $log.Contains('Backend stderr captured')) { throw 'Missing backend stderr' }
        $log = Invoke-HelperCase '--install-language-toolchains "Python,CSharp,CCpp,Java,Php"' $code $code 'PowerShell stdout captured'
        if (-not $log.Contains('PowerShell stderr captured')) { throw 'Missing PowerShell stderr' }
    }
    $null = Invoke-HelperCase '--unknown' 0 1 'Unknown setup helper action'
    $null = Invoke-HelperCase '--install-language-toolchains' 0 1 'Missing language selection'
    $null = Invoke-HelperCase '--install-user-integrations unexpected' 0 1 'Unexpected integration arguments'
    Remove-Item -LiteralPath (Join-Path $testRoot 'rustic-agent-backend.exe')
    $null = Invoke-HelperCase '--install-user-integrations' 0 1 'Setup helper failed:'
    Write-Host 'PASS: GUI subsystem, console-free child launches, logs, arguments and exit codes'
}
finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force
}
