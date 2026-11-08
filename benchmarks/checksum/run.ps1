param(
    [Parameter(Mandatory)]
    [string]$KotlinCompiler,
    [string]$MammothCompiler = (Join-Path $PSScriptRoot '../../target/release/mammoth.exe'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../../build/benchmarks'),
    [ValidateRange(1, 30)]
    [int]$CompileSamples = 5,
    [ValidateRange(1, 30)]
    [int]$ColdSamples = 9,
    [ValidateRange(1, 10)]
    [int]$WarmProcesses = 3
)

$ErrorActionPreference = 'Stop'
$KotlinCompiler = (Resolve-Path -LiteralPath $KotlinCompiler).Path
$MammothCompiler = (Resolve-Path -LiteralPath $MammothCompiler).Path
$kotlinHome = Split-Path -Parent (Split-Path -Parent $KotlinCompiler)
$stdlib = (Resolve-Path -LiteralPath (Join-Path $kotlinHome 'lib/kotlin-stdlib.jar')).Path
$java = (Get-Command java -CommandType Application | Select-Object -First 1).Source
$javac = (Get-Command javac -CommandType Application | Select-Object -First 1).Source
$run = Join-Path $OutputDirectory ('checksum-' + [Guid]::NewGuid().ToString('N'))
$run = (New-Item -ItemType Directory -Force -Path $run).FullName
$languages = @('Mammoth', 'Java', 'Kotlin')
$samples = [Collections.Generic.List[object]]::new()
$lastOutputs = @{}

function Add-Sample($language, $phase, $sample, $milliseconds) {
    $samples.Add([pscustomobject]@{
        Language = $language
        Phase = $phase
        Sample = $sample
        Milliseconds = [double]$milliseconds
    })
}

for ($sample = 0; $sample -lt $CompileSamples; $sample++) {
    for ($position = 0; $position -lt $languages.Count; $position++) {
        $language = $languages[($sample + $position) % $languages.Count]
        $destination = (New-Item -ItemType Directory -Path (Join-Path $run "$language-$sample")).FullName
        $watch = [Diagnostics.Stopwatch]::StartNew()
        switch ($language) {
            'Mammoth' { & $MammothCompiler (Join-Path $PSScriptRoot 'MammothChecksum.mam') $destination | Out-Null }
            'Java' { & $javac --release 17 -d $destination (Join-Path $PSScriptRoot 'JavaChecksum.java') | Out-Null }
            'Kotlin' { & $KotlinCompiler -jvm-target 17 -d $destination (Join-Path $PSScriptRoot 'KotlinChecksum.kt') | Out-Null }
        }
        $watch.Stop()
        if ($LASTEXITCODE -ne 0) { throw "$language compilation failed." }
        Add-Sample $language 'Compile' $sample $watch.Elapsed.TotalMilliseconds
        $lastOutputs[$language] = $destination
    }
}

$classPath = (@($languages | ForEach-Object { $lastOutputs[$_] }) + $stdlib) -join [IO.Path]::PathSeparator
$runnerOutput = (New-Item -ItemType Directory -Path (Join-Path $run 'runner')).FullName
& $javac --release 17 -cp $classPath -d $runnerOutput (Join-Path $PSScriptRoot 'Runner.java')
if ($LASTEXITCODE -ne 0) { throw 'Could not compile the benchmark runner.' }
$classPath = $runnerOutput + [IO.Path]::PathSeparator + $classPath

$coldChecksum = $null
for ($sample = 0; $sample -lt $ColdSamples; $sample++) {
    for ($position = 0; $position -lt $languages.Count; $position++) {
        $language = $languages[($sample + $position) % $languages.Count]
        $watch = [Diagnostics.Stopwatch]::StartNew()
        $output = & $java -cp $classPath Runner $language cold
        $watch.Stop()
        if ($LASTEXITCODE -ne 0 -or $output -notmatch '^\d+$') { throw "$language cold run failed." }
        if ($null -eq $coldChecksum) { $coldChecksum = $output }
        if ($output -cne $coldChecksum) { throw "$language returned a different cold checksum." }
        Add-Sample $language 'Cold' $sample $watch.Elapsed.TotalMilliseconds
    }
}

$warmChecksum = $null
for ($fork = 0; $fork -lt $WarmProcesses; $fork++) {
    for ($position = 0; $position -lt $languages.Count; $position++) {
        $language = $languages[($fork + $position) % $languages.Count]
        $output = @(& $java -cp $classPath Runner $language warm)
        if ($LASTEXITCODE -ne 0 -or $output.Count -ne 11) { throw "$language warmed run failed." }
        if ($null -eq $warmChecksum) { $warmChecksum = $output[-1] }
        if ($output[-1] -cne $warmChecksum) { throw "$language returned a different warmed checksum." }
        for ($sample = 0; $sample -lt 10; $sample++) {
            Add-Sample $language 'Warm' ($fork * 10 + $sample) ([double]$output[$sample] / 1000000)
        }
    }
}

$samples | Export-Csv -NoTypeInformation -Path (Join-Path $run 'samples.csv')
$samples | Group-Object Language,Phase | ForEach-Object {
    $values = @($_.Group.Milliseconds | Sort-Object)
    $middle = [int][Math]::Floor($values.Count / 2)
    $median = if ($values.Count % 2) { $values[$middle] } else { ($values[$middle - 1] + $values[$middle]) / 2 }
    [pscustomobject]@{
        Language = $_.Group[0].Language
        Phase = $_.Group[0].Phase
        Samples = $values.Count
        MedianMs = [Math]::Round($median, 3)
        MinMs = [Math]::Round($values[0], 3)
        MaxMs = [Math]::Round($values[-1], 3)
    }
} | Sort-Object Phase,Language | Format-Table -AutoSize

"Cold checksum: $coldChecksum"
"Warmed checksum aggregate: $warmChecksum"
"Raw samples: $(Join-Path $run 'samples.csv')"
