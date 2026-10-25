; Inno Setup script for the Windows installer. Built by .github/workflows/release.yml:
;   iscc /DAppVersion=1.2.3 /DAppUrl=https://github.com/owner/tidewater /DSourceExe=path\to\tidewater.exe /DOutDir=out /DOutName=tidewater-1.2.3-windows-x64-setup packaging\windows\tidewater.iss
; Per-user install (no administrator needed). No game data is installed.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\tidewater.exe"
#endif
#ifndef AppUrl
  #define AppUrl ""
#endif
#ifndef OutDir
  #define OutDir "."
#endif
#ifndef OutName
  #define OutName "tidewater-setup"
#endif

[Setup]
AppId={{6B0C2D7E-4F1A-4D55-9B7E-7D1D1C5A2E01}
AppName=Tidewater
AppVersion={#AppVersion}
AppPublisher=Tidewater contributors
AppPublisherURL={#AppUrl}
DefaultDirName={autopf}\Tidewater
DefaultGroupName=Tidewater
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutDir}
OutputBaseFilename={#OutName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=Tidewater
; The app icon (drawn by packaging\icon\render.sh), next to this script. tidewater.exe has no icon resource of its own
; yet, so the shortcuts and the Apps list use the installed .ico; the running window sets its own (SDL_SetWindowIcon).
SetupIconFile=tidewater.ico
UninstallDisplayIcon={app}\tidewater.ico
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "tidewater.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Tidewater"; Filename: "{app}\tidewater.exe"; IconFilename: "{app}\tidewater.ico"
Name: "{group}\Uninstall Tidewater"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Tidewater"; Filename: "{app}\tidewater.exe"; IconFilename: "{app}\tidewater.ico"; Tasks: desktopicon

; No skipifsilent: after the in-game "Update now" (a silent run) this starts the game again.
[Run]
Filename: "{app}\tidewater.exe"; Description: "Start Tidewater"; Flags: nowait postinstall
