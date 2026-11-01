[CmdletBinding()]
param([switch]$FunctionsOnly)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Find-InnoCompiler {
    param([string]$Compiler)
    if (-not [string]::IsNullOrWhiteSpace($Compiler) -and $Compiler -ine 'ISCC.exe') {
        if (-not [IO.Path]::IsPathRooted($Compiler) -or -not (Test-Path -LiteralPath $Compiler -PathType Leaf)) {
            throw 'The custom compiler must be an existing absolute ISCC.exe path.'
        }
        return (Get-Item -LiteralPath $Compiler).FullName
    }
    $command = Get-Command ISCC.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) { return $command.Source }
    # The editor may have started before an existing installation changed PATH.
    $candidates = @()
    foreach ($base in @($env:LOCALAPPDATA, $env:ProgramFiles, ${env:ProgramFiles(x86)})) {
        if ($base) {
            $candidates += Join-Path $base 'Programs\Inno Setup 6\ISCC.exe'
            $candidates += Join-Path $base 'Inno Setup 6\ISCC.exe'
        }
    }
    foreach ($key in @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1'
    )) {
        $entry = Get-ItemProperty -LiteralPath $key -Name InstallLocation -ErrorAction SilentlyContinue
        if ($entry -and $entry.InstallLocation) {
            $candidates += Join-Path $entry.InstallLocation 'ISCC.exe'
        }
    }
    foreach ($path in $candidates) {
        if (Test-Path -LiteralPath $path -PathType Leaf) { return $path }
    }
}

function Install-InnoCompiler {
    if (-not $env:LOCALAPPDATA) { throw 'LOCALAPPDATA is unavailable.' }
    $root = Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6'
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('rustic-inno-' + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $temporary | Out-Null
    try {
        $installer = Join-Path $temporary 'innosetup.exe'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -UseBasicParsing -TimeoutSec 300 `
            -Uri 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' `
            -OutFile $installer
        $signature = Get-AuthenticodeSignature -LiteralPath $installer
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'CN=Jordan Russell(?:,|$)') {
            throw 'The Inno Setup download does not have a valid Jordan Russell signature.'
        }
        $process = Start-Process -FilePath $installer -ArgumentList @(
            '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER', "/DIR=`"$root`""
        ) -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Inno Setup installation failed with exit code $($process.ExitCode)." }
        $compiler = Join-Path $root 'ISCC.exe'
        if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) { throw 'Installation did not produce ISCC.exe.' }
        return $compiler
    } finally {
        Remove-Item -LiteralPath $temporary -Recurse -Force
    }
}

function Get-InnoUserPath {
    param([string]$UserPath, [string]$Directory)
    $entries = @($userPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $found = @($entries | Where-Object {
        [Environment]::ExpandEnvironmentVariables($_).Trim().Trim('"').TrimEnd('\') -ieq $directory.TrimEnd('\')
    }).Count -gt 0
    if ($found) { return $userPath }
    if ([string]::IsNullOrEmpty($userPath)) { return $directory }
    return $userPath.TrimEnd(';') + ';' + $directory
}

function Add-InnoUserPath {
    param([string]$Compiler)
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $newPath = Get-InnoUserPath $userPath (Split-Path -Parent $Compiler)
    if ($newPath -cne $userPath) {
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        # Tell Explorer to refresh the environment for future applications.
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class RusticInnoEnvironment {
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wParam,
        string lParam, uint flags, uint timeout, out UIntPtr result);
}
'@
        $result = [UIntPtr]::Zero
        [RusticInnoEnvironment]::SendMessageTimeout([IntPtr]0xffff, 0x001A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result) | Out-Null
    }
}

function Ensure-InnoCompiler {
    param([string]$Compiler)
    $resolved = Find-InnoCompiler $Compiler
    if (-not $resolved) { $resolved = Install-InnoCompiler }
    Add-InnoUserPath $resolved
    return $resolved
}

if (-not $FunctionsOnly) {
    try {
        [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
        $ProgressPreference = 'SilentlyContinue'
        Ensure-InnoCompiler $env:RUSTIC_INNO_COMPILER
    } catch {
        [Console]::Error.WriteLine($_.Exception.Message)
        exit 1
    }
}
