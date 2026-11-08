param(
    [ValidateSet('build', 'runClient', 'smokeTest', 'verifyMod')]
    [string]$Task = 'build',
    [string]$JavaHome = '',
    [string]$Compiler = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Test-Java21([string]$HomePath) {
    if (!$HomePath) { return $false }
    $release = Join-Path $HomePath 'release'
    if (!(Test-Path -LiteralPath $release)) { return $false }
    foreach ($tool in @('java.exe', 'javac.exe', 'jar.exe')) {
        if (!(Test-Path -LiteralPath (Join-Path $HomePath "bin\$tool"))) { return $false }
    }
    return [bool](Select-String -LiteralPath $release -Pattern '^JAVA_VERSION="21\.' -Quiet)
}

if ($JavaHome -and !(Test-Java21 $JavaHome)) { throw 'JavaHome must point to a Java 21 JDK.' }
if (!$JavaHome -and (Test-Java21 $env:JAVA_HOME)) { $JavaHome = $env:JAVA_HOME }
$toolRoot = Join-Path $PSScriptRoot '.tools'
$jdkRoot = Join-Path $toolRoot 'jdk21'
if (!$JavaHome -and (Test-Path -LiteralPath $jdkRoot)) {
    foreach ($candidate in Get-ChildItem -LiteralPath $jdkRoot -Directory) {
        if (Test-Java21 $candidate.FullName) { $JavaHome = $candidate.FullName; break }
    }
}
if (!$JavaHome) {
    if ($env:OS -ne 'Windows_NT' -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
        throw 'Set JavaHome to a Java 21 JDK. Automatic setup currently supports Windows x64.'
    }
    Write-Host 'Preparing a local Java 21 JDK from Eclipse Adoptium...'
    New-Item -ItemType Directory -Path $toolRoot -Force | Out-Null
    $releases = Invoke-RestMethod -Uri 'https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture=x64&image_type=jdk&os=windows&vendor=eclipse'
    $package = $releases[0].binary.package
    $archive = Join-Path $toolRoot 'jdk21.zip'
    Invoke-WebRequest -Uri $package.link -OutFile $archive
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $package.checksum) {
        throw 'The Java download failed its SHA256 check.'
    }
    Expand-Archive -LiteralPath $archive -DestinationPath $jdkRoot -Force
    foreach ($candidate in Get-ChildItem -LiteralPath $jdkRoot -Directory) {
        if (Test-Java21 $candidate.FullName) { $JavaHome = $candidate.FullName; break }
    }
    if (!$JavaHome) { throw 'The downloaded archive did not contain a Java 21 JDK.' }
}

if (!$Compiler) { $Compiler = Join-Path $PSScriptRoot '..\..\target\release\mammoth.exe' }
$Compiler = [IO.Path]::GetFullPath($Compiler)
if (!(Test-Path -LiteralPath $Compiler)) { throw "Build Mammoth first or pass -Compiler <path>: $Compiler" }
$oldJavaHome = $env:JAVA_HOME
$oldPath = $env:Path
Push-Location $PSScriptRoot
try {
    $env:JAVA_HOME = $JavaHome
    $env:Path = "$(Join-Path $JavaHome 'bin');$env:Path"
    & (Join-Path $PSScriptRoot 'gradlew.bat') --no-daemon "-PmammothCompiler=$Compiler" $Task
    if ($LASTEXITCODE -ne 0) { throw "Gradle task failed: $Task (exit $LASTEXITCODE)" }
} finally {
    $env:JAVA_HOME = $oldJavaHome
    $env:Path = $oldPath
    Pop-Location
}
