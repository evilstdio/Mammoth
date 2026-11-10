# Removes a Mammoth install made by install.ps1: the folder, its PATH entry
# and its entry in Settings > Apps. Run it from the install folder.
#
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1 [-Quiet]
#
# Without -Quiet it asks first and reports the result in a dialog, which is
# how Settings > Apps starts it.

param([switch]$Quiet)

$ErrorActionPreference = 'Stop'
$InstallDir = $PSScriptRoot
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Mammoth'
if (-not $Quiet) { Add-Type -AssemblyName System.Windows.Forms }

function Finish([int]$code, [string]$message) {
    if ($Quiet) {
        if ($message) { Write-Host $message }
    } else {
        $icon = if ($code -eq 0) { 'Information' } else { 'Error' }
        [void][Windows.Forms.MessageBox]::Show($message, 'Uninstall Mammoth', 'OK', $icon)
    }
    exit $code
}

try {
    if (-not (Test-Path (Join-Path $InstallDir 'mammoth-install.txt'))) {
        Finish 1 "$InstallDir does not look like a Mammoth install, so nothing was removed."
    }
    if (-not $Quiet) {
        $answer = [Windows.Forms.MessageBox]::Show(
            "Remove Mammoth from $InstallDir?`n`nThis deletes the folder, including the examples in it, and takes Mammoth off your PATH. Your own .mam files elsewhere are not touched.",
            'Uninstall Mammoth', 'YesNo', 'Question')
        if ($answer -ne 'Yes') { exit 2 }
    }
    $bin = Join-Path $InstallDir 'bin'

    $key = Get-Item 'HKCU:\Environment'
    $current = $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    if ($current) {
        $entries = @($current -split ';' | Where-Object { $_ -ne '' })
        $kept = @($entries | Where-Object { $_.TrimEnd('\') -ine $bin.TrimEnd('\') })
        if ($kept.Count -ne $entries.Count) {
            Set-ItemProperty -Path 'HKCU:\Environment' -Name 'Path' -Value ($kept -join ';') -Type $key.GetValueKind('Path')
        }
    }

    if (Test-Path $UninstallKey) {
        $location = (Get-ItemProperty $UninstallKey).InstallLocation
        if ($location -and $location.TrimEnd('\') -ieq $InstallDir.TrimEnd('\')) {
            Remove-Item -Path $UninstallKey -Recurse
        }
    }

    Set-Location $env:TEMP
    Remove-Item -LiteralPath $InstallDir -Recurse -Force
    Finish 0 'Mammoth is uninstalled.'
} catch {
    Finish 1 "Uninstalling Mammoth failed: $($_.Exception.Message)"
}
