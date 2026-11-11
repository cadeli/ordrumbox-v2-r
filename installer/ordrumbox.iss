; The Windows setup of orDrumbox, built by Inno Setup 6 in CI (`.github/workflows/ci.yml`,
; job `windows-exe`) from the `dist/` folder of the release zip:
;
;   ISCC.exe /DAppVersion=<version> installer\ordrumbox.iss
;
; It installs the app, and the CLAP plugin as an option, each with its own `assets/`: both
; read the folder next to them. The plugin goes in a folder of its own under the CLAP folder,
; so that its `assets/` sits beside no other plugin. Uninstalling removes what the setup
; installed only: the user data folder (%APPDATA%\orDrumbox) stays.

#ifndef AppVersion
  #error Pass the version: ISCC.exe /DAppVersion=<version> installer\ordrumbox.iss
#endif

[Setup]
; Never change it: a new version updates the one installed through it.
AppId={{9DA1263B-B4FA-4B82-96AA-475C50833C71}
AppName=orDrumbox
AppVersion={#AppVersion}
AppVerName=orDrumbox {#AppVersion}
AppPublisherURL=https://www.ordrumbox.com
AppUpdatesURL=https://github.com/cadeli/ordrumbox-v2-r/releases
SourceDir=..
DefaultDirName={autopf}\orDrumbox
DefaultGroupName=orDrumbox
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
LicenseFile=dist\LICENSE
OutputDir=setup
OutputBaseFilename=ordrumbox-v{#AppVersion}-windows-x64-setup
UninstallDisplayIcon={app}\ordrumbox.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Types]
Name: "full"; Description: "orDrumbox and its CLAP plugin"
Name: "app"; Description: "orDrumbox only"
Name: "custom"; Description: "Custom"; Flags: iscustom

[Components]
Name: "app"; Description: "orDrumbox"; Types: full app custom; Flags: fixed
Name: "clap"; Description: "CLAP plugin (in Common Files\CLAP\orDrumbox)"; Types: full

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[InstallDelete]
; The `assets/` of a previous version goes first: a sample or a file it no longer has must
; not stay behind. Nothing writes there (the user's files are in the user data folder).
Type: filesandordirs; Name: "{app}\assets"
Type: filesandordirs; Name: "{commoncf64}\CLAP\orDrumbox\assets"; Components: clap

[Files]
Source: "dist\ordrumbox.exe"; DestDir: "{app}"; Components: app; Flags: ignoreversion
Source: "dist\LICENSE"; DestDir: "{app}"; Components: app
Source: "dist\assets\*"; DestDir: "{app}\assets"; Components: app; Flags: recursesubdirs ignoreversion
Source: "dist\orDrumbox.clap"; DestDir: "{commoncf64}\CLAP\orDrumbox"; Components: clap; Flags: ignoreversion
Source: "dist\assets\*"; DestDir: "{commoncf64}\CLAP\orDrumbox\assets"; Components: clap; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{group}\orDrumbox"; Filename: "{app}\ordrumbox.exe"
Name: "{group}\Uninstall orDrumbox"; Filename: "{uninstallexe}"
Name: "{autodesktop}\orDrumbox"; Filename: "{app}\ordrumbox.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ordrumbox.exe"; Description: "Start orDrumbox"; Flags: nowait postinstall skipifsilent
