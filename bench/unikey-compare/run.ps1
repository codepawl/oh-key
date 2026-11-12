<#
Head-to-head benchmark: UniKey's ukengine against oh-key on one Telex corpus.

  run.ps1 -UniKeySource <folder with ukengine .cpp files>

Compiles the UniKey side with your own cl.exe command line (no makefile or CMake
from the download is run), builds the Rust side in release, replays the corpus
through both and prints the table. See README.md.
#>
param(
    [Parameter(Mandatory = $true)][string]$UniKeySource,
    [string]$OutputFolder = (Join-Path $PSScriptRoot 'out')
)

$ErrorActionPreference = 'Stop'
# High priority is inherited by the replays, which keeps other work on the machine out of the timings.
(Get-Process -Id $PID).PriorityClass = 'High'
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$cargo = Join-Path $HOME '.cargo\bin\cargo.exe'
$target = 'x86_64-pc-windows-msvc'
$env:CARGO_TARGET_DIR = Join-Path $repositoryRoot 'target\bench'
$releaseFolder = Join-Path $env:CARGO_TARGET_DIR "$target\release\examples"

New-Item -ItemType Directory -Force $OutputFolder | Out-Null
$objectFolder = Join-Path $OutputFolder 'obj'
New-Item -ItemType Directory -Force $objectFolder | Out-Null

function Invoke-VisualStudioCompile {
    $vswhere = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe'
    $installation = & $vswhere -latest -products * -property installationPath
    $vcvars = Join-Path $installation 'VC\Auxiliary\Build\vcvars64.bat'
    $sources = 'byteio', 'charset', 'convert', 'data', 'error', 'inputproc', 'mactab', 'pattern', 'ukengine', 'unikey', 'usrkeymap' |
        ForEach-Object { '"' + (Join-Path $UniKeySource "$_.cpp") + '"' }
    $benchSource = Join-Path $PSScriptRoot 'unikey_bench.cpp'
    $executable = Join-Path $OutputFolder 'unikey_bench.exe'
    $shimHeader = Join-Path $PSScriptRoot 'windows_shim.h'
    # UNIKEYHOOK makes the library headers export symbols instead of importing them from a DLL.
    $compile = "cl /nologo /O2 /EHsc /MT /DUNIKEYHOOK /W0 /FI`"$shimHeader`" /I `"$UniKeySource`" `"$benchSource`" $($sources -join ' ') " +
        "/Fo`"$objectFolder\\`" /Fe`"$executable`""
    cmd /c "`"$vcvars`" >nul && $compile" | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'UniKey side failed to compile' }
}

function Invoke-Cargo {
    param([string[]]$Arguments)
    & $cargo @Arguments --release --target $target
    if ($LASTEXITCODE -ne 0) { throw "cargo failed: $Arguments" }
}

Invoke-VisualStudioCompile
Push-Location $repositoryRoot
try {
    Invoke-Cargo @('build', '-p', 'oh-key-engine', '--examples')
}
finally {
    Pop-Location
}

$keysFile = Join-Path $OutputFolder 'corpus.keys'
$expectedFile = Join-Path $OutputFolder 'corpus.expected'
$unikeyText = Join-Path $OutputFolder 'unikey.txt'
$unikeyTiming = Join-Path $OutputFolder 'unikey.timing'
$ohkeyText = Join-Path $OutputFolder 'ohkey.txt'
$ohkeyTiming = Join-Path $OutputFolder 'ohkey.timing'

& (Join-Path $releaseFolder 'compare_corpus.exe') $keysFile $expectedFile
& (Join-Path $OutputFolder 'unikey_bench.exe') $keysFile $unikeyText $unikeyTiming
& (Join-Path $releaseFolder 'compare.exe') $keysFile $ohkeyText $ohkeyTiming
& (Join-Path $releaseFolder 'compare_report.exe') $keysFile $expectedFile $unikeyText $unikeyTiming $ohkeyText $ohkeyTiming
