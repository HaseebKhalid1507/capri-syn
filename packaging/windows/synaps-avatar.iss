; synaps-avatar's Windows installer (Inno Setup 6). Per user, no admin. Built by build.ps1 (CI: the
; `windows-installer` job, an artifact); docs/WINDOWS.md says what it does.
;
; Her art is never in here: the pack is his. The wizard's "Her pack" page (unattended: /PACK="C:\dir")
; takes a pack folder on his disk and copies it to {localappdata}\synaps-avatar\packs\capri, where
; she looks by default. Without one, the folder is made empty for him to fill. Nothing is downloaded.
;
; Unattended: setup.exe /VERYSILENT /SUPPRESSMSGBOXES [/PACK="C:\packs\capri"] [/TASKS=autostart]

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Build
  #define Build "local"
#endif
#ifndef Exe
  #define Exe "..\..\target\release\synaps-avatar.exe"
#endif
#ifndef IconDir
  #define IconDir "build"
#endif
; Her AppUserModelID: src/platform/windows/notifications.rs uses the same.
#define Aumid "Synaps.Avatar"

[Setup]
AppId={{2147942F-E90C-4536-BEA2-8AB4B7F51734}
AppName=synaps-avatar
AppVersion={#AppVersion}
AppVerName=synaps-avatar {#AppVersion} ({#Build})
AppPublisher=Haseeb Khalid
AppPublisherURL=https://github.com/HaseebKhalid1507/capri-syn
PrivilegesRequired=lowest
DefaultDirName={autopf}\synaps-avatar
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; AF_UNIX (her control socket) needs 1803; GPUI's DirectComposition path 1809.
MinVersion=10.0.17763
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir=Output
OutputBaseFilename=synaps-avatar-setup-{#AppVersion}-{#Build}
SetupIconFile={#IconDir}\synaps-avatar.ico
UninstallDisplayIcon={app}\synaps-avatar.ico
UninstallDisplayName=synaps-avatar
; An upgrade closes the running one (Restart Manager), and doesn't restart it.
CloseApplications=force
RestartApplications=no

[Tasks]
Name: autostart; Description: "Start her when I sign in"; Flags: unchecked

[Files]
Source: "{#Exe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#IconDir}\synaps-avatar.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#IconDir}\synaps-avatar.png"; DestDir: "{app}"; Flags: ignoreversion
; His pack, from his disk at install time (`external`): never inside this installer.
Source: "{code:PackSource}\*"; DestDir: "{code:PackDest}"; Flags: external recursesubdirs createallsubdirs ignoreversion; Check: HavePack

[InstallDelete]
; A new pack replaces the old one whole (no stale frames from the last).
Type: filesandordirs; Name: "{code:PackDest}"; Check: HavePack

[Dirs]
; Where she looks for packs: made even without one. Removed at uninstall if empty.
Name: "{localappdata}\synaps-avatar\packs"

[Icons]
; The shortcut carries her AppUserModelID: toasts show under her name and icon.
Name: "{autoprograms}\synaps-avatar"; Filename: "{app}\synaps-avatar.exe"; WorkingDir: "{app}"; IconFilename: "{app}\synaps-avatar.ico"; AppUserModelID: "{#Aumid}"; Comment: "Capri, on your desktop"

[Registry]
; Her AUMID for toasts (she writes DisplayName herself too, if run without this installer).
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\{#Aumid}"; ValueType: string; ValueName: "DisplayName"; ValueData: "synaps-avatar"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\{#Aumid}"; ValueType: string; ValueName: "IconUri"; ValueData: "{app}\synaps-avatar.png"
; Opt-in: start at sign-in.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "synaps-avatar"; ValueData: """{app}\synaps-avatar.exe"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\synaps-avatar.exe"; Description: "Start her now"; Flags: nowait postinstall skipifsilent; Check: PackReady

[UninstallRun]
; She may be running: stop her (his own processes only: no admin).
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM synaps-avatar.exe"; Flags: runhidden; RunOnceId: "StopHer"

[UninstallDelete]
; What she wrote herself: her log and state. His settings (%APPDATA%\synaps-avatar\settings.toml)
; and any pack he put in packs\ himself stay.
Type: files; Name: "{localappdata}\synaps-avatar\synaps-avatar.log"
Type: files; Name: "{localappdata}\synaps-avatar\synaps-avatar.old.log"
Type: files; Name: "{localappdata}\synaps-avatar\state.json"
Type: files; Name: "{localappdata}\synaps-avatar\state.json.tmp"
Type: files; Name: "{localappdata}\synaps-avatar\sessions.json"
Type: files; Name: "{localappdata}\synaps-avatar\sessions.json.tmp"
Type: dirifempty; Name: "{localappdata}\synaps-avatar\packs"
Type: dirifempty; Name: "{localappdata}\synaps-avatar"

[Code]
var
  PackPage: TInputQueryWizardPage;

function PackDest(Param: String): String;
begin
  Result := ExpandConstant('{localappdata}\synaps-avatar\packs\capri');
end;

function PackSource(Param: String): String;
begin
  Result := '';
  if PackPage <> nil then
    Result := RemoveBackslashUnlessRoot(Trim(PackPage.Values[0]));
end;

{ A pack to copy: given, and not the destination itself. }
function HavePack: Boolean;
begin
  Result := (PackSource('') <> '') and (CompareText(ExpandFileName(PackSource('')), PackDest('')) <> 0);
end;

function PackReady: Boolean;
begin
  Result := FileExists(PackDest('') + '\sprites.json');
end;

{ Empty: fine (no pack). Else it must be a pack folder. }
function PackError(P: String): String;
begin
  Result := '';
  if P = '' then
    exit;
  if not DirExists(P) then
    Result := 'There is no folder ' + P + '.'
  else if not FileExists(AddBackslash(P) + 'sprites.json') then
    Result := P + ' has no sprites.json: it is not a pack.';
end;

procedure BrowseClick(Sender: TObject);
var
  Dir: String;
begin
  Dir := PackPage.Values[0];
  if BrowseForFolder('Her pack folder (the one with sprites.json):', Dir, False) then
    PackPage.Values[0] := Dir;
end;

procedure InitializeWizard;
var
  Browse: TNewButton;
begin
  PackPage := CreateInputQueryPage(wpSelectTasks, 'Her pack', 'Which pack should she wear?',
    'Her art isn''t in this installer. Pick a pack folder (one with sprites.json): Setup copies it to ' +
    PackDest('') + ', where she looks for it. Leave this empty to put one there yourself later. ' +
    'Nothing is downloaded.');
  PackPage.Add('Pack folder:', False);
  PackPage.Values[0] := ExpandConstant('{param:PACK|}');
  Browse := TNewButton.Create(PackPage);
  Browse.Parent := PackPage.Surface;
  Browse.Caption := 'Browse...';
  Browse.Left := PackPage.Edits[0].Left;
  Browse.Top := PackPage.Edits[0].Top + PackPage.Edits[0].Height + ScaleY(8);
  Browse.Width := ScaleX(90);
  Browse.Height := WizardForm.NextButton.Height;
  Browse.OnClick := @BrowseClick;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  E: String;
begin
  Result := True;
  if CurPageID = PackPage.ID then begin
    E := PackError(PackSource(''));
    if E <> '' then begin
      MsgBox(E, mbError, MB_OK);
      Result := False;
    end;
  end;
end;

{ Unattended installs skip the page: the same check, as Setup's error. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := PackError(PackSource(''));
end;
