; Built by .github/workflows/release.yml: iscc /DVersion=<x.y.z> /DExe=<path to rustcord.exe>
#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{E3ED0947-A8ED-4369-B0DF-A93F202E6BDD}
AppName=Dango
AppVersion={#Version}
AppPublisher=frstycodes
; Per-user, no admin prompt: the updater replaces Dango.exe in place, so its folder must be writable
; by the user (Program Files is not).
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\Dango
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
OutputBaseFilename=dango-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
SetupIconFile=..\..\assets\windows\app.ico
UninstallDisplayIcon={app}\Dango.exe
CloseApplications=yes

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "Dango.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Dango"; Filename: "{app}\Dango.exe"
Name: "{autodesktop}\Dango"; Filename: "{app}\Dango.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Dango.exe"; Description: "Launch Dango"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; What an update set aside while Dango was running.
Type: files; Name: "{app}\Dango.exe.old"
