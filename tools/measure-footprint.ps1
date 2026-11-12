<#
Reads the footprint of the installed, running oh-key and its files. Read only:
it never starts, stops or changes the program, and it types nothing.

  measure-footprint.ps1 [-Seconds 30] [-InstallFolder <folder>]

Prints working set, private bytes, threads, handles, CPU time over the interval
and the file sizes. Run it while oh-key sits idle in the tray.
#>
param(
    [int]$Seconds = 30,
    [string]$InstallFolder = (Join-Path $env:LOCALAPPDATA 'Programs\oh-key')
)

$ErrorActionPreference = 'Stop'
$process = Get-Process -Name 'oh-key' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $process) { throw 'oh-key is not running' }

function Get-ProcessSnapshot {
    $process.Refresh()
    [pscustomobject]@{
        WorkingSetMB = [math]::Round($process.WorkingSet64 / 1MB, 2)
        PrivateMB    = [math]::Round($process.PrivateMemorySize64 / 1MB, 2)
        Threads      = $process.Threads.Count
        Handles      = $process.HandleCount
        CpuMs        = $process.TotalProcessorTime.TotalMilliseconds
    }
}

$start = Get-ProcessSnapshot
Start-Sleep -Seconds $Seconds
$end = Get-ProcessSnapshot

Write-Host "date: $(Get-Date -Format 'yyyy-MM-dd HH:mm')"
Write-Host "process: id $($process.Id), started $($process.StartTime.ToString('yyyy-MM-dd HH:mm')), path $($process.Path)"
Write-Host "working set: $($end.WorkingSetMB) MB"
Write-Host "private bytes: $($end.PrivateMB) MB"
Write-Host "threads: $($end.Threads)"
Write-Host "handles: $($end.Handles)"
Write-Host "cpu time over $Seconds s: $([math]::Round($end.CpuMs - $start.CpuMs, 1)) ms"
foreach ($file in Get-ChildItem $InstallFolder -File) {
    Write-Host "file: $($file.Name) $($file.Length) bytes ($([math]::Round($file.Length / 1KB, 1)) KB)"
}
