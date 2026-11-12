# oh-key releases

This repository holds the built files of oh-key, a Vietnamese typing app for Windows and macOS. It contains no source code. The source is not public yet.

## Download

Get the latest files from the [latest release](https://github.com/codepawl/oh-key-releases/releases/latest).

| File | For |
| --- | --- |
| [`oh-key-windows-x64.exe`](https://github.com/codepawl/oh-key-releases/releases/latest/download/oh-key-windows-x64.exe) | Windows on Intel or AMD, 64-bit |
| [`oh-key-windows-x86.exe`](https://github.com/codepawl/oh-key-releases/releases/latest/download/oh-key-windows-x86.exe) | Windows, 32-bit |
| [`oh-key-windows-arm64.exe`](https://github.com/codepawl/oh-key-releases/releases/latest/download/oh-key-windows-arm64.exe) | Windows on ARM |
| [`oh-key-windows-<arch>.zip`](https://github.com/codepawl/oh-key-releases/releases/latest) | The app and the command line tool in one zip |
| [`oh-key-macos.zip`](https://github.com/codepawl/oh-key-releases/releases/latest/download/oh-key-macos.zip) | macOS input method, Apple silicon and Intel |
| [`SHA256SUMS.txt`](https://github.com/codepawl/oh-key-releases/releases/latest/download/SHA256SUMS.txt) | SHA-256 checksum of every file above |

## Install on Windows

Run this command in PowerShell. It needs no administrator rights.

```powershell
irm https://raw.githubusercontent.com/codepawl/oh-key-releases/main/install.ps1 | iex
```

The script downloads the right file for your processor, checks its SHA-256 and installs it in `%LOCALAPPDATA%\Programs\oh-key`.

## Verify a download

1. Download the file and `SHA256SUMS.txt` into the same folder.
2. Run `Get-FileHash .\oh-key-windows-x64.exe -Algorithm SHA256` in PowerShell.
3. Compare the hash with the line for that file in `SHA256SUMS.txt`. The two values must be equal.

On macOS or Linux, run `shasum -a 256 -c SHA256SUMS.txt --ignore-missing`. It prints `OK` for each file that matches.

## Updates

oh-key checks this repository for a new release. You choose in the settings whether it installs updates, only tells you, or does nothing.
