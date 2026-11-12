# Measures the trim work: starts its OWN instance of the given exe (hook off, separate instance name), waits for idle,
# counts CPU time and context switches over 60 s, then opens and closes the settings window. It shows a window,
# so run it only on a machine you are not using. Usage: measure-trim.ps1 -Exe <path> -Label <name> -Instance <name>
param([string]$Exe, [string]$Label, [string]$Instance)
$env:OHKEY_NO_HOOK = '1'
$env:OHKEY_INSTANCE = $Instance
Add-Type -Namespace Win -Name Api -MemberDefinition '[DllImport("user32.dll")] public static extern bool PostMessageW(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l);'

function Snapshot($process, $name) {
    $process.Refresh()
    [pscustomobject]@{
        Stage = $name
        WorkingSetMB = [math]::Round($process.WorkingSet64 / 1MB, 2)
        PrivateMB = [math]::Round($process.PrivateMemorySize64 / 1MB, 2)
        Threads = $process.Threads.Count
        Handles = $process.HandleCount
    }
}
function SwitchTable($processId) {
    $table = @{}
    Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter "IDProcess=$processId" | ForEach-Object { $table[$_.IDThread] = [int64]$_.ContextSwitchesPersec }
    $table
}
function SwitchDelta($start, $end) {
    $sum = 0
    foreach ($id in $start.Keys) { if ($end.ContainsKey($id)) { $sum += $end[$id] - $start[$id] } }
    $sum
}

$process = Start-Process -FilePath $Exe -PassThru
Write-Host "started pid $($process.Id)"
try {
    Start-Sleep -Seconds 6
    $idle = Snapshot $process 'idle'
    $cpuStart = $process.TotalProcessorTime.TotalMilliseconds
    $switchStart = SwitchTable $process.Id
    Start-Sleep -Seconds 60
    $process.Refresh()
    $cpuMs = [math]::Round($process.TotalProcessorTime.TotalMilliseconds - $cpuStart, 1)
    $switches = SwitchDelta $switchStart (SwitchTable $process.Id)
    $idleAfter = Snapshot $process 'idle after 60s'

    $second = Start-Process -FilePath $Exe -PassThru
    Start-Sleep -Seconds 4
    $opened = Snapshot $process 'panel open'
    $process.Refresh()
    $window = $process.MainWindowHandle
    if ($window -ne [IntPtr]::Zero) { [Win.Api]::PostMessageW($window, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null } else { Write-Host 'no panel window found' }
    Start-Sleep -Seconds 3
    $closed = Snapshot $process 'panel closed +3s'
    Start-Sleep -Seconds 10
    $closedLater = Snapshot $process 'panel closed +13s'

    $process.Refresh(); Write-Host ('modules: ' + ($process.Modules | ForEach-Object ModuleName | Sort-Object) -join ' ')
    Write-Host "== $Label"
    $idle, $idleAfter, $opened, $closed, $closedLater | Format-Table -AutoSize | Out-String | Write-Host
    Write-Host "cpu ms over 60s: $cpuMs ; context switches over 60s: $switches"
}
finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
}
