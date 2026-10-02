; Fliq Windows installer (spec 8.2). Build with Inno Setup 6:
;   iscc /DAppVersion=0.2.0 /DSourceDir=..\build\windows\x64\runner\Release windows_installer\fliq.iss
; Code signing: configure a SignTool named "fliqsign" in Inno Setup (see README) and pass
;   /DSign=1. Unsigned installers trigger SmartScreen warnings.

#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\build\windows\x64\runner\Release"
#endif

[Setup]
AppId={{6C1F3E0B-5A0B-4F7E-9B7B-0F1100000001}
AppName=Fliq
AppVersion={#AppVersion}
AppPublisher=Fliq
DefaultDirName={autopf}\Fliq
DefaultGroupName=Fliq
DisableProgramGroupPage=yes
OutputBaseFilename=FliqSetup-{#AppVersion}
OutputDir=..\build\installer
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Admin rights are needed to add the firewall rule.
PrivilegesRequired=admin
UninstallDisplayIcon={app}\fliq.exe
WizardStyle=modern
#ifdef Sign
SignTool=fliqsign
SignedUninstaller=yes
#endif

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{autoprograms}\Fliq"; Filename: "{app}\fliq.exe"
Name: "{autodesktop}\Fliq"; Filename: "{app}\fliq.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Run]
; Inbound rule on Private AND Public profiles: hotspot networks are usually "Public".
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Fliq"""; Flags: runhidden; StatusMsg: "Configuring Windows Firewall..."
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Fliq"" dir=in action=allow protocol=TCP program=""{app}\fliq.exe"" enable=yes profile=private,public"; Flags: runhidden
Filename: "{app}\fliq.exe"; Description: "Start Fliq"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Fliq"""; Flags: runhidden; RunOnceId: "RemoveFliqFirewallRule"
