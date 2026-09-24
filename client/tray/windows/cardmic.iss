; Cardmic for Windows: per-user installer (no administrator rights needed).
; Built by CI:  iscc /DAppVersion=0.6.0 tray\windows\cardmic.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6F3C2A8E-4B1D-4E0A-9C7B-2D5E8A1F0C63}
AppName=Cardmic
AppVersion={#AppVersion}
AppVerName=Cardmic {#AppVersion}
AppPublisher=The Cardmic Authors
AppPublisherURL=https://github.com/Yueze/Cardmic
AppSupportURL=https://github.com/Yueze/Cardmic/issues
DefaultDirName={localappdata}\Programs\Cardmic
DisableProgramGroupPage=yes
DisableDirPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\target\windows
OutputBaseFilename=Cardmic-Setup
SetupIconFile=..\assets\Cardmic.ico
UninstallDisplayIcon={app}\Cardmic.exe
UninstallDisplayName=Cardmic
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=force

[Files]
Source: "..\..\target\release\cardmic-app.exe"; DestDir: "{app}"; DestName: "Cardmic.exe"; Flags: ignoreversion
; The command-line client, for scripts and troubleshooting.
Source: "..\..\target\release\cardmic.exe"; DestDir: "{app}\cli"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Cardmic"; Filename: "{app}\Cardmic.exe"

[Run]
Filename: "{app}\Cardmic.exe"; Description: "Start Cardmic"; Flags: nowait postinstall skipifsilent
; A silent install is the app updating itself: start it again.
Filename: "{app}\Cardmic.exe"; Parameters: "--updated"; Flags: nowait; Check: WizardSilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM Cardmic.exe"; Flags: runhidden; RunOnceId: "StopCardmic"

[Registry]
; The app adds this itself ("Start with Windows"); remove it with the app.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Cardmic"; Flags: uninsdeletevalue dontcreatekey
