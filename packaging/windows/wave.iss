; Built by .github/workflows/release.yml: iscc /DVersion=<x.y.z> /DExe=<path to rustcord.exe>
#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{E3ED0947-A8ED-4369-B0DF-A93F202E6BDD}
AppName=Wave
AppVersion={#Version}
AppPublisher=frstycodes
; Per-user, no admin prompt: the updater replaces Wave.exe in place, so its folder must be writable
; by the user (Program Files is not).
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\Wave
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
OutputBaseFilename=wave-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
UninstallDisplayIcon={app}\Wave.exe
CloseApplications=yes

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "Wave.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Wave"; Filename: "{app}\Wave.exe"
Name: "{autodesktop}\Wave"; Filename: "{app}\Wave.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Wave.exe"; Description: "Launch Wave"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; What an update set aside while Wave was running.
Type: files; Name: "{app}\Wave.exe.old"
