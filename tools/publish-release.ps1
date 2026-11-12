# Publishes an existing release of codepawl/oh-key to the public codepawl/oh-key-releases repo.
# Use it when the RELEASES_TOKEN secret is not set and the release workflow skipped that step.
#   .\tools\publish-release.ps1 -Tag v0.2.0
#
# It needs the GitHub CLI signed in (`gh auth login`) with read access to codepawl/oh-key and
# write access to codepawl/oh-key-releases. An existing release with the same tag gets its assets
# replaced, so the script is safe to run again.
param(
    [Parameter(Mandatory = $true)][string]$Tag
)
$ErrorActionPreference = 'Stop'

$privateRepo = 'codepawl/oh-key'
$publicRepo = 'codepawl/oh-key-releases'

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw 'The GitHub CLI (gh) is required. Install it from https://cli.github.com and run "gh auth login".'
}

function Invoke-Gh {
    & gh @args
    if ($LASTEXITCODE -ne 0) { throw "gh $($args -join ' ') failed with exit code $LASTEXITCODE." }
}

function ConvertTo-VersionNumber([string]$tag) {
    $parsed = $null
    if ([version]::TryParse($tag.TrimStart('v'), [ref]$parsed)) { return $parsed }
    return $null
}

# True when the public repo already has a latest release with a higher version than $tag.
function Test-IsOlderThanLatest([string]$tag) {
    $latestTag = & gh release view --repo $publicRepo --json tagName --jq .tagName 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $latestTag) { return $false }
    $latest = ConvertTo-VersionNumber $latestTag
    $mine = ConvertTo-VersionNumber $tag
    return ($null -ne $latest) -and ($null -ne $mine) -and ($latest -gt $mine)
}

$workFolder = Join-Path ([IO.Path]::GetTempPath()) ('oh-key-publish-' + [Guid]::NewGuid())
$assetFolder = Join-Path $workFolder 'assets'
New-Item -ItemType Directory -Path $assetFolder | Out-Null
try {
    Write-Host "Downloading the assets of $Tag from $privateRepo..."
    Invoke-Gh release download $Tag --repo $privateRepo --dir $assetFolder
    # Written here, not copied from the private release: its generated notes name pull requests,
    # authors and private links.
    $notesPath = Join-Path $workFolder 'release-notes.md'
    $notes = "oh-key $($Tag.TrimStart('v'))`n`nBuilt files and SHA256SUMS.txt. The app updates itself from this page.`n"
    Set-Content -Path $notesPath -Value $notes -Encoding utf8

    $assetPaths = (Get-ChildItem $assetFolder -File).FullName
    if (-not $assetPaths) { throw "Release $Tag has no assets." }

    & gh release view $Tag --repo $publicRepo *> $null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Release $Tag exists in $publicRepo. Replacing its assets..."
        Invoke-Gh release upload $Tag @assetPaths --clobber --repo $publicRepo
    }
    else {
        Write-Host "Creating release $Tag in $publicRepo..."
        # GitHub calls the newest release by creation date "latest", which is what the updater
        # reads. Republishing an older tag must not take that place.
        $latestFlag = if (Test-IsOlderThanLatest $Tag) { '--latest=false' } else { '--latest' }
        Invoke-Gh release create $Tag @assetPaths --repo $publicRepo --title $Tag --notes-file $notesPath $latestFlag
    }
    Write-Host "Published $Tag to https://github.com/$publicRepo/releases/tag/$Tag"
}
finally {
    Remove-Item $workFolder -Recurse -Force -ErrorAction SilentlyContinue
}
