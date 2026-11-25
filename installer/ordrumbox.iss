; The Windows setup of orDrumbox, built by Inno Setup 6 in CI (`.github/workflows/ci.yml`,
; job `windows-exe`) from the `dist/` folder of the release zip:
;
;   ISCC.exe /DAppVersion=<version> installer\ordrumbox.iss
;
; It installs the app with its `assets/`, which it reads from the folder next to it.
; Uninstalling removes what the setup installed only: the user data folder
; (%APPDATA%\orDrumbox) stays.

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
SetupIconFile=installer\ordrumbox-setup.ico
UninstallDisplayIcon={app}\ordrumbox.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[InstallDelete]
; The `assets/` of a previous version goes first: a sample or a file it no longer has must
; not stay behind. Nothing writes there (the user's files are in the user data folder).
Type: filesandordirs; Name: "{app}\assets"

[Files]
Source: "dist\ordrumbox.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "dist\LICENSE"; DestDir: "{app}"
Source: "dist\THIRD-PARTY-LICENSES.txt"; DestDir: "{app}"
Source: "installer\ordrumbox.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "dist\assets\*"; DestDir: "{app}\assets"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{group}\orDrumbox"; Filename: "{app}\ordrumbox.exe"; IconFilename: "{app}\ordrumbox.ico"
Name: "{group}\Uninstall orDrumbox"; Filename: "{uninstallexe}"
Name: "{autodesktop}\orDrumbox"; Filename: "{app}\ordrumbox.exe"; IconFilename: "{app}\ordrumbox.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\ordrumbox.exe"; Description: "Start orDrumbox"; Flags: nowait postinstall skipifsilent

[Code]
{ On an upgrade only (the app is already installed through this setup), a note after the welcome
  page: the new demos are added by the app, a factory reset restores all of them as shipped. }
var
  UpgradeNotePage: TOutputMsgWizardPage;

function IsUpgrade(): Boolean;
begin
  Result := RegKeyExists(HKLM, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#SetupSetting("AppId")}_is1')
    or RegKeyExists(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#SetupSetting("AppId")}_is1');
end;

procedure InitializeWizard();
begin
  UpgradeNotePage := CreateOutputMsgPage(wpWelcome, 'Upgrading orDrumbox',
    'Your songs and sounds are kept.',
    'The new demos are added at the first start of the new version. To get all the demos as ' +
    'shipped, updated ones included, run Help > Factory reset... in orDrumbox: your files are ' +
    'backed up first, and File > Restore user data brings them back.');
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = UpgradeNotePage.ID) and not IsUpgrade();
end;
