; Salvaê installer. Build with:
;   ISCC.exe /DMyAppVersion=1.1.1 packaging\installer.iss
; Produces packaging\Salvae-Setup.exe (per-machine, into Program Files).

#define MyAppName "Salvaê"
#define MyAppExeName "Salvae.exe"
#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif

[Setup]
; A stable AppId so upgrades replace the same install (do not change it).
AppId={{8F3A1C2E-5B6D-4E7F-9A0B-1C2D3E4F5A6B}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=Salvaê
DefaultDirName={autopf}\Salvae
DefaultGroupName=Salvaê
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
OutputDir=.
OutputBaseFilename=Salvae-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Branded wizard art (mascot on the app's dark background) instead of the
; default setup-box image. Pre-sized to the wizard panel ratio, so stretch is
; safe. Inno Setup 6.3+ accepts PNG here.
WizardImageFile=wizard-image.png
WizardSmallImageFile=wizard-small.png
WizardImageStretch=yes
PrivilegesRequired=admin
; Lets a silent update close the running app and reopen it.
AppMutex=Salvae
CloseApplications=yes
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; Under Start Menu\Programs — indexed by Windows search.
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"

[Run]
; Interactive install: optional "launch now" checkbox.
Filename: "{app}\{#MyAppExeName}"; Description: "Abrir o {#MyAppName}"; \
  Flags: nowait postinstall skipifsilent runasoriginaluser
; Silent install (auto-update): relaunch as the normal (non-elevated) user.
Filename: "{app}\{#MyAppExeName}"; Flags: nowait runasoriginaluser; Check: WizardSilent
