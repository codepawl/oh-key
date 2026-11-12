<#
Mixed Vietnamese / English benchmark: English words, brands, code and mixed sentences
typed in Telex mode, replayed through UniKey's ukengine and through oh-key.

  run-mixed.ps1 [-Runs 5]

Needs unikey_bench.exe from run.ps1 (run that first). Prints a markdown report with
the median ns per key of the runs and the wrong tokens per category. See README.md.
#>
param(
    [string]$OutputFolder = (Join-Path $PSScriptRoot 'out\mixed'),
    [int]$Runs = 5
)

$ErrorActionPreference = 'Stop'
(Get-Process -Id $PID).PriorityClass = 'High'
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$cargo = Join-Path $HOME '.cargo\bin\cargo.exe'
$target = 'x86_64-pc-windows-msvc'
$env:CARGO_TARGET_DIR = Join-Path $repositoryRoot 'target\bench'
$examples = Join-Path $env:CARGO_TARGET_DIR "$target\release\examples"
$unikeyBench = Join-Path $PSScriptRoot 'out\unikey_bench.exe'
if (-not (Test-Path $unikeyBench)) { throw 'Run run.ps1 first: it builds unikey_bench.exe' }

& $cargo build --release --target $target -p oh-key-engine --examples
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
New-Item -ItemType Directory -Force $OutputFolder | Out-Null

function Get-OutputPath([string]$name) { Join-Path $OutputFolder $name }

function Get-MedianNanosecondsPerKey([string]$timingFile) {
    [double](Get-Content $timingFile).Split(' ')[0]
}

function Get-Median([double[]]$values) {
    $sorted = $values | Sort-Object
    $middle = [int][math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2 -eq 1) { return $sorted[$middle] }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

$keys = Get-OutputPath 'mixed.keys'
$expected = Get-OutputPath 'mixed.expected'
$categories = Get-OutputPath 'mixed.categories'
& (Join-Path $examples 'mixed_corpus.exe') $keys $expected $categories

$unikeySamples = @()
$ohkeySamples = @()
foreach ($run in 1..$Runs) {
    & $unikeyBench $keys (Get-OutputPath 'unikey.txt') (Get-OutputPath 'unikey.timing') | Out-Null
    & (Join-Path $examples 'compare.exe') $keys (Get-OutputPath 'ohkey.txt') (Get-OutputPath 'ohkey.timing') | Out-Null
    $unikeySamples += Get-MedianNanosecondsPerKey (Get-OutputPath 'unikey.timing')
    $ohkeySamples += Get-MedianNanosecondsPerKey (Get-OutputPath 'ohkey.timing')
}
"$(Get-Median $unikeySamples) 0 0" | Set-Content (Get-OutputPath 'unikey.median')
"$(Get-Median $ohkeySamples) 0 0" | Set-Content (Get-OutputPath 'ohkey.median')
Write-Host "UniKey runs: $unikeySamples"
Write-Host "oh-key runs: $ohkeySamples"

& (Join-Path $examples 'mixed_report.exe') $keys $expected $categories (Get-OutputPath 'unikey.txt') (Get-OutputPath 'ohkey.txt') (Get-OutputPath 'unikey.median') (Get-OutputPath 'ohkey.median') |
    Tee-Object (Get-OutputPath 'report.md')
