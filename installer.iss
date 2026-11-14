#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{6F1D2C1E-4B7A-4E36-9B0F-3D8E5A2C7B41}
AppName=Deck Display
AppVersion={#AppVersion}
DefaultDirName={autopf}\Deck Display
DefaultGroupName=Deck Display
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=dist
OutputBaseFilename=DeckDisplaySetup
SetupIconFile=assets\icon.ico
UninstallDisplayIcon={app}\deckdisplay.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: "startup"; Description: "Start with Windows"
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "target\release\deckdisplay.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Deck Display"; Filename: "{app}\deckdisplay.exe"
Name: "{autodesktop}\Deck Display"; Filename: "{app}\deckdisplay.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Deck Display"; ValueData: """{app}\deckdisplay.exe"" --background"; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\deckdisplay.exe"; Description: "Open Deck Display"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/im deckdisplay.exe /f"; Flags: runhidden; RunOnceId: "StopDeckDisplay"
