# Installs Mammoth for the current user. No administrator rights are needed.
#
# The setup program runs this script from the folder it unpacks to, beside
# mammoth.zip. It can also be run by hand from an unpacked release:
#
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-InstallDir <folder>] [-NoPath] [-NoRegister] [-Quiet]

param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Mammoth'),
    [string]$Payload = '',
    [switch]$NoPath,
    [switch]$NoRegister,
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'
$Marker = 'mammoth-install.txt'
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Mammoth'

function Finish([int]$code, [string]$message) {
    if ($message) { Write-Host $message }
    if (-not $Quiet) { Read-Host 'Press Enter to close' | Out-Null }
    exit $code
}

function Send-EnvironmentChange {
    # Tells Explorer and new terminals that PATH changed, without a sign-out.
    if (-not ('Mammoth.Environment' -as [type])) {
        Add-Type -Namespace Mammoth -Name Environment -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern System.IntPtr SendMessageTimeout(System.IntPtr hWnd, uint Msg, System.UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out System.UIntPtr lpdwResult);
'@
    }
    $result = [UIntPtr]::Zero
    [void][Mammoth.Environment]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
}

function Add-UserPath([string]$directory) {
    # Read PATH unexpanded and keep its registry type, so entries such as
    # %USERPROFILE%\bin still work afterwards.
    $key = Get-Item 'HKCU:\Environment'
    $current = $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    $entries = @($current -split ';' | Where-Object { $_ -ne '' })
    if ($entries | Where-Object { $_.TrimEnd('\') -ieq $directory.TrimEnd('\') }) { return $false }
    $kind = if ($current -eq '') { 'ExpandString' } else { $key.GetValueKind('Path') }
    $updated = (@($entries) + $directory) -join ';'
    Set-ItemProperty -Path 'HKCU:\Environment' -Name 'Path' -Value $updated -Type $kind
    Send-EnvironmentChange
    return $true
}

try {
    if (-not $Payload) { $Payload = Join-Path $PSScriptRoot 'mammoth.zip' }
    $InstallDir = [IO.Path]::GetFullPath($InstallDir)
    Write-Host "Installing Mammoth to $InstallDir"

    if (Test-Path $InstallDir) {
        $items = @(Get-ChildItem -LiteralPath $InstallDir -Force)
        if ($items.Count -gt 0 -and -not (Test-Path (Join-Path $InstallDir $Marker))) {
            Finish 1 "That folder already has other files in it, so nothing was changed. Choose an empty folder with -InstallDir."
        }
    }

    # Unpack beside the target first, so a failure leaves any old install alone.
    $staging = "$InstallDir.new"
    if (Test-Path $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
    if (Test-Path $Payload) {
        Expand-Archive -LiteralPath $Payload -DestinationPath $staging
    } elseif (Test-Path (Join-Path $PSScriptRoot 'bin\mammoth.exe')) {
        # Run from an unpacked release folder.
        New-Item -ItemType Directory -Path $staging | Out-Null
        Get-ChildItem -LiteralPath $PSScriptRoot -Force | Copy-Item -Destination $staging -Recurse
    } else {
        Finish 1 "Could not find mammoth.zip next to this script."
    }
    # A release zip holds one top folder; install its contents.
    $inner = @(Get-ChildItem -LiteralPath $staging -Directory)
    if ($inner.Count -eq 1 -and (Test-Path (Join-Path $inner[0].FullName 'bin\mammoth.exe'))) {
        $root = $inner[0].FullName
    } else {
        $root = $staging
    }
    $compiler = Join-Path $root 'bin\mammoth.exe'
    if (-not (Test-Path $compiler)) { Finish 1 "The package is missing bin\mammoth.exe." }
    $version = (& $compiler --version) -replace '^mammoth ', ''
    if ($LASTEXITCODE -ne 0) { Finish 1 "The packaged compiler did not start." }

    if (Test-Path $InstallDir) { Remove-Item -LiteralPath $InstallDir -Recurse -Force }
    Move-Item -LiteralPath $root -Destination $InstallDir
    if (Test-Path $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
    Set-Content -LiteralPath (Join-Path $InstallDir $Marker) -Encoding UTF8 -Value @(
        "Mammoth $version",
        "Installed $(Get-Date -Format s)",
        "This file marks the folder as a Mammoth install. uninstall.ps1 removes the folder."
    )

    $bin = Join-Path $InstallDir 'bin'
    if (-not $NoPath) {
        if (Add-UserPath $bin) { Write-Host "Added $bin to your PATH." }
        else { Write-Host "$bin is already on your PATH." }
    }

    if (-not $NoRegister) {
        $uninstall = Join-Path $InstallDir 'uninstall.ps1'
        $size = [int]((Get-ChildItem -LiteralPath $InstallDir -Recurse -File | Measure-Object Length -Sum).Sum / 1KB)
        New-Item -Path $UninstallKey -Force | Out-Null
        $values = @{
            DisplayName     = "Mammoth $version"
            DisplayVersion  = $version
            Publisher       = 'Mammoth'
            InstallLocation = $InstallDir
            UninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$uninstall`""
        }
        foreach ($name in $values.Keys) { Set-ItemProperty -Path $UninstallKey -Name $name -Value $values[$name] }
        Set-ItemProperty -Path $UninstallKey -Name NoModify -Value 1 -Type DWord
        Set-ItemProperty -Path $UninstallKey -Name NoRepair -Value 1 -Type DWord
        Set-ItemProperty -Path $UninstallKey -Name EstimatedSize -Value $size -Type DWord
    }

    $hint = if ($NoPath) { "& `"$bin\mammoth.exe`"" } else { 'mammoth' }
    Finish 0 @"

Mammoth $version is installed. Java is included, so nothing else is needed.
Open a new terminal and try:

    $hint --version
    $hint "$InstallDir\examples\greet.mam"

Remove it from Settings > Apps > Installed apps, or run uninstall.ps1 in $InstallDir.
"@
} catch {
    Finish 1 "Installing Mammoth failed: $($_.Exception.Message)"
}
