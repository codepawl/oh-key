# Dev install of the oh-key TSF service: copy the DLLs into Program Files, register them,
# add the keyboard under Vietnamese and turn on the diagnostics log. Run elevated after
# building ohkey_tip.dll for x86_64- and i686-pc-windows-msvc into target\lead (CARGO_TARGET_DIR).
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
Start-Transcript -Path "$repository\target\register.log" -Force | Out-Null
try {
    $installFolder = 'C:\Program Files\oh-key'
    $builds = @{
        'x64' = @{ Source = "$repository\target\lead\x86_64-pc-windows-msvc\release\ohkey_tip.dll"; Register = "$env:windir\System32\regsvr32.exe" }
        'x86' = @{ Source = "$repository\target\lead\i686-pc-windows-msvc\release\ohkey_tip.dll"; Register = "$env:windir\SysWOW64\regsvr32.exe" }
    }
    foreach ($architecture in $builds.Keys) {
        $folder = Join-Path $installFolder $architecture
        New-Item -ItemType Directory -Force -Path $folder | Out-Null
        $target = Join-Path $folder 'ohkey_tip.dll'
        Get-ChildItem $folder -Filter '*.old*' | Remove-Item -Force -ErrorAction SilentlyContinue
        if (Test-Path $target) { Rename-Item $target ('ohkey_tip.dll.old' + [guid]::NewGuid().ToString('N').Substring(0, 6)) }
        Copy-Item -Force $builds[$architecture].Source $target
        $process = Start-Process -FilePath $builds[$architecture].Register -ArgumentList '/s', "`"$target`"" -Wait -PassThru
        Write-Output "regsvr32 $architecture exit code: $($process.ExitCode)"
    }

    $logFile = 'C:\Users\Public\Documents\oh-key.log'
    if (-not (Test-Path $logFile)) { New-Item -ItemType File $logFile | Out-Null }
    $tip ='042A:{6D5A7F1E-3B2C-4E8A-9C41-7A1E5B0D2F63}{B8E0C4A2-5F17-4D93-A6B1-2C9E7D4F8A15}'
    $languages = Get-WinUserLanguageList
    $vietnamese = $languages | Where-Object LanguageTag -eq 'vi'
    if (-not $vietnamese) {
        $languages.Add('vi')
        $vietnamese = $languages | Where-Object LanguageTag -eq 'vi'
        $vietnamese.InputMethodTips.Clear()
    }
    if ($vietnamese.InputMethodTips -notcontains $tip) { $vietnamese.InputMethodTips.Add($tip) }
    Set-WinUserLanguageList $languages -Force
    Get-WinUserLanguageList | Format-List LanguageTag, InputMethodTips | Out-String | Write-Output
    Write-Output 'DONE'
}
catch {
    Write-Output "FAILED: $_"
}
finally {
    Stop-Transcript | Out-Null
}
