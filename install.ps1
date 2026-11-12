# Downloads the latest oh-key release into %LOCALAPPDATA%\Programs\oh-key. No admin rights needed.
#   .\install.ps1            download and unpack only
#   .\install.ps1 -Register  also register the text service (needs admin, shows a UAC prompt)
#
# The repository is private, so this needs the GitHub CLI (https://cli.github.com) signed in
# with `gh auth login`. Once the repository is public the same release files work without it.
param([switch]$Register)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw 'The GitHub CLI (gh) is required while the repository is private. Install it from https://cli.github.com, run "gh auth login", then run this script again.'
}

$architecture = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x64' }
    'ARM64' { 'arm64' }
    'x86'   { 'x86' }
    default { throw "Unsupported processor architecture: $($env:PROCESSOR_ARCHITECTURE)" }
}

$installFolder = Join-Path $env:LOCALAPPDATA 'Programs\oh-key'
$downloadFolder = Join-Path ([IO.Path]::GetTempPath()) ('oh-key-' + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $downloadFolder | Out-Null
try {
    $archiveName = "oh-key-windows-$architecture.zip"
    Write-Host "Downloading the latest $archiveName..."
    gh release download --repo codepawl/oh-key --pattern $archiveName --pattern 'SHA256SUMS.txt' --dir $downloadFolder
    if ($LASTEXITCODE -ne 0) { throw 'gh release download failed.' }

    $archivePath = Join-Path $downloadFolder $archiveName
    $checksumLine = Get-Content (Join-Path $downloadFolder 'SHA256SUMS.txt') | Where-Object { $_ -match [regex]::Escape($archiveName) }
    $expected = ($checksumLine -split '\s+')[0]
    $actual = (Get-FileHash $archivePath -Algorithm SHA256).Hash
    if (-not $expected -or $expected -ne $actual) {
        throw "Checksum mismatch: expected $expected, got $actual"
    }

    New-Item -ItemType Directory -Path $installFolder -Force | Out-Null
    Expand-Archive $archivePath -DestinationPath $installFolder -Force
}
finally {
    Remove-Item $downloadFolder -Recurse -Force -ErrorAction SilentlyContinue
}

$dllPath = Join-Path $installFolder 'ohkey_tip.dll'
if ($Register) {
    Start-Process regsvr32.exe -ArgumentList "/s `"$dllPath`"" -Verb RunAs -Wait
    Write-Host 'oh-key is registered. Add it under Settings > Time & language > Language & region > Vietnamese > Keyboards.'
}
else {
    Write-Host "oh-key is unpacked in $installFolder."
    Write-Host "To register the text service, run again with -Register, or run as admin: regsvr32 `"$dllPath`""
}
