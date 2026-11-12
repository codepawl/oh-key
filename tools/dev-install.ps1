# Dev install of the oh-key tray app: stop the running oh-key.exe (from Program Files or
# %LOCALAPPDATA%\Programs\oh-key), copy the release build from target\lead into
# %LOCALAPPDATA%\Programs\oh-key and start it again. No elevation, no registry changes beyond
# refreshing an existing "start with Windows" value. Build first:
#   $env:CARGO_TARGET_DIR = "target/lead"
#   cargo build -p oh-key-app --release --target x86_64-pc-windows-msvc
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$installFolder = Join-Path $env:LOCALAPPDATA 'Programs\oh-key'
$appSource = "$repository\target\lead\x86_64-pc-windows-msvc\release\oh-key.exe"
$appTarget = Join-Path $installFolder 'oh-key.exe'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

if (-not (Test-Path $appSource)) { throw "Build first: $appSource does not exist." }

Get-Process -Name 'oh-key' -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -like "$installFolder\*" -or $_.Path -like "$env:ProgramFiles\oh-key\*" } |
    Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

New-Item -ItemType Directory -Force -Path $installFolder | Out-Null
Copy-Item -Force $appSource $appTarget

if ($null -ne (Get-ItemProperty -Path $runKey -Name 'oh-key' -ErrorAction SilentlyContinue)) {
    Set-ItemProperty -Path $runKey -Name 'oh-key' -Value ('"' + $appTarget + '"')
}

# explorer.exe starts the app with the normal rights of the signed-in user.
Start-Process explorer.exe -ArgumentList "`"$appTarget`""
Write-Output "Installed $appTarget"
