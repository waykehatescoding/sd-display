#ifndef AppVersion
  #define AppVersion "0.1.2"
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
CloseApplications=no

[Tasks]
Name: "startup"; Description: "Start with Windows"
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "defender"; Description: "Add a Windows Security exclusion for deckdisplay.exe (asks for admin)"; Flags: unchecked

[Files]
Source: "target\release\deckdisplay.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Deck Display"; Filename: "{app}\deckdisplay.exe"
Name: "{autodesktop}\Deck Display"; Filename: "{app}\deckdisplay.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Deck Display"; ValueData: """{app}\deckdisplay.exe"" --background"; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "powershell.exe"; Parameters: "-NoProfile -NonInteractive -Command ""Add-MpPreference -ExclusionPath '{code:ExePath}'"""; Verb: runas; Flags: shellexec runhidden waituntilterminated; Tasks: defender; Check: not WizardSilent; StatusMsg: "Adding Windows Security exclusion..."
Filename: "{app}\deckdisplay.exe"; Description: "Open Deck Display"; Flags: nowait postinstall skipifsilent
Filename: "{app}\deckdisplay.exe"; Parameters: "--background"; Flags: nowait; Check: WizardSilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/im deckdisplay.exe /f"; Flags: runhidden; RunOnceId: "StopDeckDisplay"
Filename: "powershell.exe"; Parameters: "-NoProfile -NonInteractive -Command ""Remove-MpPreference -ExclusionPath '{code:ExePath}'"""; Verb: runas; Flags: shellexec runhidden waituntilterminated; Tasks: defender; RunOnceId: "RemoveDefenderExclusion"

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/im deckdisplay.exe /f', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
  Result := '';
end;

function ExePath(Param: String): String;
begin
  Result := ExpandConstant('{app}\deckdisplay.exe');
  StringChangeEx(Result, '''', '''''', True);
end;
