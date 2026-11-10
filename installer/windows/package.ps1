# Builds the Windows release of Mammoth:
#
#   build\dist\mammoth-<version>-windows-x64\        the installed layout
#   build\dist\mammoth-<version>-windows-x64.zip     the same, zipped
#   build\dist\mammoth-<version>-setup.exe           a setup program for it
#
# The release carries its own Java runtime, made with jlink from a JDK 17 or
# later, so people who install it need neither Rust nor Java.
#
#   powershell -ExecutionPolicy Bypass -File installer\windows\package.ps1 [-Jdk <JDK folder>] [-NoSetup]

param(
    [string]$Jdk = '',
    [switch]$NoSetup
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$here = $PSScriptRoot

# Java modules in the bundled runtime: all of Java SE, the jar tool and
# compiler that packaging and --check-java use, and the JDK modules programs
# commonly need (TLS curves, the HTTP server, zip file systems, Unsafe).
$modules = @(
    'java.se', 'jdk.jartool', 'jdk.compiler', 'jdk.zipfs', 'jdk.crypto.ec', 'jdk.crypto.cryptoki',
    'jdk.httpserver', 'jdk.unsupported', 'jdk.charsets', 'jdk.localedata', 'jdk.net',
    'jdk.management', 'jdk.naming.dns', 'jdk.security.auth', 'jdk.xml.dom', 'jdk.jfr', 'jdk.jdwp.agent'
)

function Step([string]$text) { Write-Host "==> $text" }

function Find-Jdk {
    if ($Jdk) { return (Resolve-Path $Jdk).Path }
    if ($env:JAVA_HOME -and (Test-Path (Join-Path $env:JAVA_HOME 'jmods'))) { return $env:JAVA_HOME }
    $java = Get-Command java.exe -ErrorAction SilentlyContinue
    if ($java) {
        $found = Split-Path (Split-Path (Resolve-Path $java.Source).Path)
        if (Test-Path (Join-Path $found 'jmods')) { return $found }
    }
    throw 'Could not find a JDK with jmods. Pass -Jdk <folder> or set JAVA_HOME to a JDK 17 or later.'
}

$version = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "mammoth-$version-windows-x64"
$dist = Join-Path $repo 'build\dist'
$stage = Join-Path $dist $name
$jdkHome = Find-Jdk
$jlink = Join-Path $jdkHome 'bin\jlink.exe'
if (-not (Test-Path $jlink)) { throw "$jdkHome has no bin\jlink.exe." }
$javaVersion = (Get-Content (Join-Path $jdkHome 'release') | Where-Object { $_ -like 'JAVA_VERSION=*' }) -replace 'JAVA_VERSION=|"', ''

Step "Building the compiler ($version)"
Push-Location $repo
try {
    cargo build --release --offline
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
} finally { Pop-Location }

Step "Assembling $stage"
if (Test-Path $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $stage 'bin') | Out-Null
Copy-Item (Join-Path $repo 'target\release\mammoth.exe') (Join-Path $stage 'bin')

Step "Making the Java $javaVersion runtime from $jdkHome"
# No --compress: the compiler reads classes straight from lib\modules and does
# not decompress jimage entries. The zip and the setup program compress anyway.
& $jlink --module-path (Join-Path $jdkHome 'jmods') --add-modules ($modules -join ',') `
    --strip-debug --no-man-pages --no-header-files --compress=0 `
    --output (Join-Path $stage 'runtime')
if ($LASTEXITCODE -ne 0) { throw 'jlink failed.' }

# Only files Git tracks: examples\fabric-demo also holds Gradle caches with
# Minecraft's own JARs, which must not be redistributed.
Push-Location $repo
try { $tracked = @(git ls-files -- examples) } finally { Pop-Location }
if ($LASTEXITCODE -ne 0 -or $tracked.Count -eq 0) { throw 'git ls-files found no examples.' }
foreach ($file in $tracked) {
    $target = Join-Path $stage $file
    New-Item -ItemType Directory -Force -Path (Split-Path $target) | Out-Null
    Copy-Item -LiteralPath (Join-Path $repo $file) -Destination $target
}
foreach ($file in 'LICENSE', 'README.md', 'CHANGELOG.md') { Copy-Item (Join-Path $repo $file) $stage }
foreach ($file in 'install.ps1', 'install.cmd', 'uninstall.ps1') { Copy-Item (Join-Path $here $file) $stage }
$vendor = (Get-Content (Join-Path $jdkHome 'release') | Where-Object { $_ -like 'IMPLEMENTOR=*' }) -replace 'IMPLEMENTOR=|"', ''
Set-Content -LiteralPath (Join-Path $stage 'THIRD-PARTY.txt') -Encoding UTF8 -Value @"
Mammoth is licensed under Apache-2.0 WITH LLVM-exception. See LICENSE.

The runtime folder is a Java $javaVersion runtime made with jlink from the
OpenJDK build by $vendor. OpenJDK is licensed under the GNU General Public
License, version 2, with the Classpath Exception. The license and notices for
each module are in runtime\legal.
"@

Step 'Checking the assembled compiler uses its bundled runtime'
$probe = Join-Path $env:TEMP "mammoth-package-check-$PID"
New-Item -ItemType Directory -Force -Path $probe | Out-Null
try {
    $saved = $env:PATH, $env:JAVA_HOME
    # Hide every other Java so only the bundled runtime can answer.
    $env:PATH = "$env:SystemRoot\System32;$env:SystemRoot"
    $env:JAVA_HOME = ''
    $out = & (Join-Path $stage 'bin\mammoth.exe') (Join-Path $stage 'examples\greet.mam')
    if ($LASTEXITCODE -ne 0 -or ($out -join "`n") -notmatch 'Hello, Maya!') { throw "The packaged compiler could not run greet.mam: $out" }
    & (Join-Path $stage 'bin\mammoth.exe') --jar (Join-Path $stage 'examples\greet.mam') (Join-Path $probe 'greet.jar')
    if ($LASTEXITCODE -ne 0) { throw 'The packaged compiler could not build a JAR.' }
} finally {
    $env:PATH, $env:JAVA_HOME = $saved
    Remove-Item -LiteralPath $probe -Recurse -Force -ErrorAction SilentlyContinue
}

Step "Zipping $name.zip"
$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -LiteralPath $zip }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip, [IO.Compression.CompressionLevel]::Optimal, $true)

if (-not $NoSetup) {
    Step "Making mammoth-$version-setup.exe with IExpress"
    $iexpress = Join-Path $env:SystemRoot 'System32\iexpress.exe'
    if (-not (Test-Path $iexpress)) { throw 'IExpress is not available. Use -NoSetup to make only the zip.' }
    $work = Join-Path $dist 'setup-work'
    if (Test-Path $work) { Remove-Item -LiteralPath $work -Recurse -Force }
    New-Item -ItemType Directory -Path $work | Out-Null
    Copy-Item $zip (Join-Path $work 'mammoth.zip')
    foreach ($file in 'install.ps1', 'install.cmd') { Copy-Item (Join-Path $here $file) $work }
    Copy-Item (Join-Path $repo 'LICENSE') $work
    $setup = Join-Path $dist "mammoth-$version-setup.exe"
    if (Test-Path $setup) { Remove-Item -LiteralPath $setup }
    $sed = Join-Path $work 'setup.sed'
    Set-Content -LiteralPath $sed -Encoding ASCII -Value @"
[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=1
HideExtractAnimation=0
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=%InstallPrompt%
DisplayLicense=%DisplayLicense%
FinishMessage=%FinishMessage%
TargetName=%TargetName%
FriendlyName=%FriendlyName%
AppLaunched=%AppLaunched%
PostInstallCmd=%PostInstallCmd%
AdminQuietInstCmd=%AdminQuietInstCmd%
UserQuietInstCmd=%UserQuietInstCmd%
SourceFiles=SourceFiles
[Strings]
InstallPrompt=
DisplayLicense=
FinishMessage=
TargetName=$setup
FriendlyName=Mammoth $version Setup
AppLaunched=powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File install.ps1 -Gui
PostInstallCmd=<None>
AdminQuietInstCmd=cmd.exe /c install.cmd -Quiet
UserQuietInstCmd=cmd.exe /c install.cmd -Quiet
FILE0="mammoth.zip"
FILE1="install.ps1"
FILE2="install.cmd"
FILE3="LICENSE"
[SourceFiles]
SourceFiles0=$work\
[SourceFiles0]
%FILE0%=
%FILE1%=
%FILE2%=
%FILE3%=
"@
    $process = Start-Process -FilePath $iexpress -ArgumentList '/N', '/Q', $sed -Wait -PassThru
    if ($process.ExitCode -ne 0 -or -not (Test-Path $setup)) { throw "IExpress failed with exit code $($process.ExitCode)." }
    Remove-Item -LiteralPath $work -Recurse -Force
}

Step 'Done'
Get-ChildItem -LiteralPath $dist -File | ForEach-Object { '{0,-40} {1,8:N1} MB' -f $_.Name, ($_.Length / 1MB) }
