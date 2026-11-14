# Footage branch only: records the real oh-key typing on a hosted Windows runner.
# Never run this on a developer PC: it takes the foreground and types through a global hook.
param(
    [Parameter(Mandatory = $true)][string]$OhKeyExe,
    [Parameter(Mandatory = $true)][string]$OutFolder
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $OutFolder | Out-Null
Add-Type -Path (Join-Path $PSScriptRoot 'Typer.cs')
Add-Type -AssemblyName System.Windows.Forms

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FootageWindow {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr window, int x, int y, int width, int height, bool repaint);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Ansi)]
    public struct DisplayMode {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string DeviceName;
        public short SpecVersion, DriverVersion, Size, DriverExtra;
        public int Fields, PositionX, PositionY, DisplayOrientation, FixedOutput;
        public short Color, Duplex, YResolution, TTOption, Collate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string FormName;
        public short LogPixels;
        public int BitsPerPel, PelsWidth, PelsHeight, DisplayFlags, DisplayFrequency;
        public int IcmMethod, IcmIntent, MediaType, DitherType, Reserved1, Reserved2, PanningWidth, PanningHeight;
    }
    [DllImport("user32.dll")] static extern bool EnumDisplaySettings(string device, int mode, ref DisplayMode devMode);
    [DllImport("user32.dll")] static extern int ChangeDisplaySettings(ref DisplayMode devMode, int flags);

    /// Returns the ChangeDisplaySettings result: 0 means the new size took.
    public static int SetResolution(int width, int height) {
        var mode = new DisplayMode();
        mode.Size = (short)Marshal.SizeOf(typeof(DisplayMode));
        EnumDisplaySettings(null, -1, ref mode);
        mode.PelsWidth = width;
        mode.PelsHeight = height;
        mode.Fields = 0x80000 | 0x100000;
        return ChangeDisplaySettings(ref mode, 0);
    }

    [DllImport("user32.dll")] static extern bool SystemParametersInfo(uint action, uint parameter, IntPtr value, uint flags);

    /// Windows' "Animation effects" switch; the runner has it off, and the mode badge follows it.
    public static bool EnableAnimations() {
        return SystemParametersInfo(0x1043, 0, new IntPtr(1), 3);
    }
}
'@

function Write-Step([string]$message) {
    Write-Host "[footage] $message"
}

function Start-Recording([string]$name) {
    $output = Join-Path $OutFolder "$name.mp4"
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = 'ffmpeg'
    $info.Arguments = "-y -loglevel error -f gdigrab -framerate 30 -draw_mouse 0 -i desktop -c:v libx264 -preset ultrafast -crf 16 -pix_fmt yuv420p `"$output`""
    $info.RedirectStandardInput = $true
    $info.UseShellExecute = $false
    $process = [System.Diagnostics.Process]::Start($info)
    Start-Sleep -Milliseconds 1500
    return $process
}

function Stop-Recording($process) {
    Start-Sleep -Milliseconds 1200
    $process.StandardInput.Write('q')
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(30000)) {
        $process.Kill()
    }
}

function Save-Screenshot([string]$name) {
    & ffmpeg -y -loglevel error -f gdigrab -i desktop -frames:v 1 (Join-Path $OutFolder "$name.png")
}

# Notepad sits at the top-left at a fixed size so long lines wrap and the reel can frame the
# whole window without panning.
$NotepadWidth = 1000
$NotepadHeight = 540

function Focus-Process($process) {
    $process.Refresh()
    $handle = $process.MainWindowHandle
    [FootageWindow]::ShowWindow($handle, 1) | Out-Null
    [FootageWindow]::MoveWindow($handle, 0, 0, $NotepadWidth, $NotepadHeight, $true) | Out-Null
    [FootageWindow]::SetForegroundWindow($handle) | Out-Null
    Start-Sleep -Milliseconds 600
}

function Open-Notepad {
    $notepad = Start-Process notepad -PassThru
    $notepad.WaitForInputIdle(10000) | Out-Null
    Start-Sleep -Seconds 1
    Focus-Process $notepad
    return $notepad
}

function Record-NotepadScene([string]$name, [string]$script, [int]$pace) {
    Write-Step "scene $name"
    $notepad = Open-Notepad
    $recording = Start-Recording $name
    [FootageTyper]::Run($script, $pace)
    [FootageTyper]::TakeEvents() | Set-Content (Join-Path $OutFolder "$name-keys.txt")
    Stop-Recording $recording
    Save-Screenshot "$name-end"
    Stop-Process -Id $notepad.Id -Force
    Start-Sleep -Milliseconds 800
}

# Desktop: as large as the runner allows, every tray icon visible, plain Notepad in a big font.
$resolutionResult = [FootageWindow]::SetResolution(1920, 1080)
Write-Step "resolution change result $resolutionResult"
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Write-Step "screen $($screen.Width)x$($screen.Height)"
$notepadKey = 'HKCU:\Software\Microsoft\Notepad'
New-Item -Force $notepadKey | Out-Null
Set-ItemProperty $notepadKey lfFaceName 'Segoe UI'
Set-ItemProperty $notepadKey iPointSize 400 -Type DWord
Set-ItemProperty $notepadKey StatusBar 0 -Type DWord
Set-ItemProperty $notepadKey fWrap 1 -Type DWord
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' EnableAutoTray 0 -Type DWord
Write-Step "animations on: $([FootageWindow]::EnableAnimations())"

$configFolder = Join-Path $env:APPDATA 'oh-key'
New-Item -ItemType Directory -Force $configFolder | Out-Null
@"
send_reports = false
start_at_sign_in = false
updates = "off"
sound_effects = true
visual_effects = true
"@ | Set-Content -Encoding utf8 (Join-Path $configFolder 'config.toml')

$env:OHKEY_ACCEPT_INJECTED_KEYS = '1'
$env:OHKEY_NO_UPDATES = '1'
Write-Step 'starting oh-key'
$ohKey = Start-Process $OhKeyExe -PassThru
Start-Sleep -Seconds 3
# Windows 11 keeps new tray icons in the overflow; promote oh-key's so the V/E icon shows.
Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue | ForEach-Object {
    $path = (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).ExecutablePath
    if ($path -and $path -like '*oh-key.exe') {
        Set-ItemProperty $_.PSPath IsPromoted 1 -Type DWord
        Write-Step "tray icon promoted"
    }
}
Start-Sleep -Seconds 2
Save-Screenshot 'desktop-start'

Record-NotepadScene 'notepad-coffee' 'Sangs nay uoongs coffee voiws boss, nhows guiwr pass wifi nhes{WAIT 1500}' 80
Record-NotepadScene 'notepad-toggle' 'Gox tieengs Vieetj {WAIT 400}{TOGGLE}{WAIT 1200}then English {WAIT 400}{TOGGLE}{WAIT 1200}roofi Vieetj tieeps{WAIT 1500}' 85
Record-NotepadScene 'notepad-reedit' 'Looix space roofi: viet {WAIT 700}{BS}{WAIT 500}j vaanx suwar dduwowcj{WAIT 1500}' 90
Record-NotepadScene 'notepad-fast' 'Tieengs Vieetj gox nhanh khoong sai, dduowngf, nguwowif, khuyeens, quyeenf, giuwax{WAIT 1500}' 45

Write-Step 'scene chrome-search'
$chromePath = 'C:\Program Files\Google\Chrome\Application\chrome.exe'
if (Test-Path $chromePath) {
    $profileFolder = Join-Path $env:RUNNER_TEMP 'chrome-profile'
    $chrome = Start-Process $chromePath -PassThru -ArgumentList @(
        "--user-data-dir=$profileFolder", '--no-first-run', '--no-default-browser-check',
        '--start-maximized', '--disable-search-engine-choice-screen', '--force-device-scale-factor=2',
        'https://www.google.com/?hl=vi'
    )
    Start-Sleep -Seconds 6
    Save-Screenshot 'chrome-before'
    $recording = Start-Recording 'chrome-search'
    [FootageTyper]::Run('caf phee suwax ddas gaafn ddaay{WAIT 2000}', 85)
    [FootageTyper]::TakeEvents() | Set-Content (Join-Path $OutFolder "chrome-search-keys.txt")
    Stop-Recording $recording
    Save-Screenshot 'chrome-search-end'
    Stop-Process -Id $chrome.Id -Force -ErrorAction SilentlyContinue
} else {
    Write-Step 'chrome not found'
}

Stop-Process -Id $ohKey.Id -Force -ErrorAction SilentlyContinue
Write-Step 'done'
