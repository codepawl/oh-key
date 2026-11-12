# Installs the latest oh-key release: the tray app (oh-key.exe) and the command line tool.
#   .\install.ps1                    install into %LOCALAPPDATA%\Programs\oh-key and start the app
#   .\install.ps1 -StartAtSignIn     also start the app every time you sign in
#
# No administrator rights are needed: everything goes into your own profile.
#
# The repository is private, so this needs the GitHub CLI (https://cli.github.com) signed in
# with `gh auth login`. Once the repository is public the same release files work without it.
param(
    [switch]$StartAtSignIn
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$installFolder = Join-Path $env:LOCALAPPDATA 'Programs\oh-key'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

function Stop-RunningApp {
    Get-Process -Name 'oh-key' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -like "$installFolder\*" -or $_.Path -like "$env:ProgramFiles\oh-key\*" } |
        Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
}

# Points the Run value at the installed exe. An existing value is refreshed even without the
# switch, so the path never goes stale when the exe moves.
function Set-StartAtSignIn {
    $command = '"' + (Join-Path $installFolder 'oh-key.exe') + '"'
    Set-ItemProperty -Path $runKey -Name 'oh-key' -Value $command
}

function Test-StartAtSignInEnabled {
    $null -ne (Get-ItemProperty -Path $runKey -Name 'oh-key' -ErrorAction SilentlyContinue)
}

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw 'The GitHub CLI (gh) is required while the repository is private. Install it from https://cli.github.com, run "gh auth login", then run this script again.'
}

$architecture = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x64' }
    'ARM64' { 'arm64' }
    'x86'   { 'x86' }
    default { throw "Unsupported processor architecture: $($env:PROCESSOR_ARCHITECTURE)" }
}

$workFolder = Join-Path ([IO.Path]::GetTempPath()) ('oh-key-' + [Guid]::NewGuid())
$stagedFolder = Join-Path $workFolder 'files'
New-Item -ItemType Directory -Path $stagedFolder | Out-Null
try {
    $archiveName = "oh-key-windows-$architecture.zip"
    Write-Host "Downloading the latest $archiveName..."
    gh release download --repo codepawl/oh-key --pattern $archiveName --pattern 'SHA256SUMS.txt' --dir $workFolder
    if ($LASTEXITCODE -ne 0) { throw 'gh release download failed.' }

    $archivePath = Join-Path $workFolder $archiveName
    $checksumLine = Get-Content (Join-Path $workFolder 'SHA256SUMS.txt') | Where-Object { $_ -match [regex]::Escape($archiveName) }
    $expected = ($checksumLine -split '\s+')[0]
    $actual = (Get-FileHash $archivePath -Algorithm SHA256).Hash
    if (-not $expected -or $expected -ne $actual) {
        throw "Checksum mismatch: expected $expected, got $actual"
    }
    Expand-Archive $archivePath -DestinationPath $stagedFolder -Force

    Stop-RunningApp
    New-Item -ItemType Directory -Path $installFolder -Force | Out-Null
    foreach ($fileName in 'oh-key.exe', 'oh-key-cli.exe') {
        Copy-Item (Join-Path $stagedFolder $fileName) (Join-Path $installFolder $fileName) -Force
    }
}
finally {
    Remove-Item $workFolder -Recurse -Force -ErrorAction SilentlyContinue
}

if ($StartAtSignIn -or (Test-StartAtSignInEnabled)) { Set-StartAtSignIn }
Start-Process (Join-Path $installFolder 'oh-key.exe')
Write-Host "oh-key is installed in $installFolder and running. Click the V in the tray to open its panel."
