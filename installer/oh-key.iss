; The one-click Windows installer, oh-key-setup.exe. One file for every Windows: it carries the
; x64, x86 and arm64 builds and installs the one that runs natively.
;
; It installs for the current user only (no administrator rights) into
; %LOCALAPPDATA%\Programs\oh-key, the same folder install.ps1 and the self-updater use, so an
; installed copy keeps updating itself in place.
;
; Built in CI by .github/workflows/release.yml:
;   ISCC.exe /DAppVersion=0.1.0 /DBuildFolder=<folder with build-x64\, build-x86\, build-arm64\> installer\oh-key.iss
; The exe lands in installer\output\oh-key-setup.exe.

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif
#ifndef BuildFolder
  #error Pass /DBuildFolder=<folder holding build-x64, build-x86 and build-arm64>
#endif

[Setup]
AppId={{6C1E3E0A-8F4B-4E57-9B0B-0E7C5F4A2D11}
AppName=oh-key
AppVersion={#AppVersion}
AppVerName=oh-key {#AppVersion}
AppPublisher=CodePawl
AppPublisherURL=https://ohkey.codepawl.com
AppSupportURL=https://ohkey.codepawl.com
AppUpdatesURL=https://ohkey.codepawl.com
DefaultDirName={userpf}\oh-key
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableWelcomePage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x86compatible x64compatible arm64
ArchitecturesInstallIn64BitMode=x64os arm64
OutputDir=output
OutputBaseFilename=oh-key-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\oh-key.exe
UninstallDisplayName=oh-key
CloseApplications=force
RestartApplications=no
LicenseFile=..\LICENSE
ShowLanguageDialog=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#BuildFolder}\build-arm64\oh-key.exe"; DestDir: "{app}"; Flags: ignoreversion; Check: IsArm64
Source: "{#BuildFolder}\build-x64\oh-key.exe"; DestDir: "{app}"; Flags: ignoreversion; Check: IsX64OS
Source: "{#BuildFolder}\build-x86\oh-key.exe"; DestDir: "{app}"; Flags: ignoreversion; Check: IsPlainX86
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion

[Registry]
; The app keeps this value in step itself at every launch; the switch in its panel removes it.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "oh-key"; ValueData: """{app}\oh-key.exe"""; Flags: uninsdeletevalue

[Icons]
Name: "{userprograms}\oh-key"; Filename: "{app}\oh-key.exe"

[Run]
Filename: "{app}\oh-key.exe"; Description: "Start oh-key"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Only the copy in this folder, never a build running from somewhere else.
Filename: "powershell.exe"; Parameters: "-NoProfile -Command ""Get-Process oh-key -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq '{app}\oh-key.exe' }} | Stop-Process -Force"""; Flags: runhidden; RunOnceId: "StopApp"

[UninstallDelete]
; Left behind by the self-updater while it swaps the exe.
Type: files; Name: "{app}\oh-key.old.exe"
Type: files; Name: "{app}\oh-key.new.exe"

[Code]
function IsPlainX86: Boolean;
begin
  Result := (not IsArm64) and (not IsX64OS);
end;
