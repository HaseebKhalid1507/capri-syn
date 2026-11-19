; capri-syn's Windows installer (Inno Setup 6): everything she needs, per user, no admin. Built by
; build.ps1 (CI: the `windows-installer` job, an artifact); docs/WINDOWS.md says what it does.
;
; Her binary (synaps-avatar.exe, the windowed build), her shortcut and toast name, then, after the files,
; the install steps install.ps1 runs too: one copy of them, packaging/windows/steps.ps1, installed in
; {app}\steps with what they read (the manifest, make-pack, the pins, her moods and persona: no art).
;   - Synaps, her brain: found as install.ps1 finds it; if it's missing or too old, the "Her brain"
;     page installs the latest release with its own installer (checked).
;   - Her pack: the "Her pack" page downloads her art and builds it on his machine (the default), copies
;     a pack folder he has, or skips. Her art is never in this installer.
;   - The Synaps daemon, now and at sign-in (no console window).
;   - Her voice, when he ticks it: the voice installer and its models.
; A step that fails doesn't fail Setup: she is installed, and the finish page says what happened in
; plain words, with the one command that tries it again. Unattended, the log says it.
;
; Unattended: setup.exe /VERYSILENT /SUPPRESSMSGBOXES [/SYNAPS=install|skip]
;             [/PACK=download|skip|"C:\packs\capri"] [/TASKS="autostart,voice"]
; (the defaults are the wizard's: install Synaps if needed, download her art, autostart on, no voice).
; install.ps1 runs it with /SYNAPS=skip /PACK=skip /BY=install.ps1: Synaps and her pack are its own
; steps, and her settings and its cache its own to remove.

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
; Her voice installer's AppId (synaps-avatar-voice.iss).
#define VoiceAppId "{B7E0CA8F-064C-4B7F-939A-41681146411C}"

[Setup]
; The AppId of every synaps-avatar install before the rename: an upgrade replaces it in place.
AppId={{2147942F-E90C-4536-BEA2-8AB4B7F51734}
AppName=capri-syn
AppVersion={#AppVersion}
AppVerName=capri-syn {#AppVersion} ({#Build})
AppPublisher=Haseeb Khalid
AppPublisherURL=https://github.com/HaseebKhalid1507/capri-syn
PrivilegesRequired=lowest
; Her binary and data keep the name synaps-avatar (her folder, %LOCALAPPDATA%\synaps-avatar).
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
OutputBaseFilename=capri-syn-setup-{#AppVersion}-{#Build}
SetupIconFile={#IconDir}\synaps-avatar.ico
UninstallDisplayIcon={app}\synaps-avatar.ico
UninstallDisplayName=capri-syn
; An upgrade closes the running one (Restart Manager), and doesn't restart it.
CloseApplications=force
RestartApplications=no

[Tasks]
Name: autostart; Description: "Start her when I sign in"
Name: voice; Description: "Her voice: talk to her out loud (downloads her voice installer and ~320 MB of models)"; Flags: unchecked

[Files]
; The install steps and what they read, in a tree like the repo's (steps.ps1 finds installer\manifest
; beside it). First: the "Her brain" page runs steps.ps1 before anything is installed.
Source: "steps.ps1"; DestDir: "{app}\steps"; Flags: ignoreversion
Source: "..\..\installer\manifest"; DestDir: "{app}\steps\installer"; Flags: ignoreversion
Source: "..\..\tools\make-pack"; DestDir: "{app}\steps\tools"; Flags: ignoreversion
Source: "..\..\tools\capricore-*.sha256"; DestDir: "{app}\steps\tools"; Flags: ignoreversion
Source: "..\..\tools\lachy-desktop-*.sha256"; DestDir: "{app}\steps\tools"; Flags: ignoreversion
Source: "..\..\tools\desktop-art.json"; DestDir: "{app}\steps\tools"; Flags: ignoreversion
Source: "..\..\packs\capri\moods.json"; DestDir: "{app}\steps\packs\capri"; Flags: ignoreversion
Source: "..\..\packs\capri\persona.md"; DestDir: "{app}\steps\packs\capri"; Flags: ignoreversion
Source: "{#Exe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#IconDir}\synaps-avatar.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#IconDir}\synaps-avatar.png"; DestDir: "{app}"; Flags: ignoreversion

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
; Start at sign-in (install.ps1's step 6 sets the same value).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "synaps-avatar"; ValueData: """{app}\synaps-avatar.exe"""; Flags: uninsdeletevalue; Tasks: autostart

[Run]
; Started from his profile folder, as install.ps1 starts her.
Filename: "{app}\synaps-avatar.exe"; WorkingDir: "{%USERPROFILE}"; Description: "Start her now"; Flags: nowait postinstall skipifsilent; Check: PackReady
; No brain yet: Synaps' Claude sign-in, with no window (steps.ps1 login): his browser opens.
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\steps\steps.ps1"" login"; Description: "Sign in to Claude, her brain (your browser opens; a Claude Pro or Max plan)"; Flags: nowait postinstall skipifsilent runhidden; Check: OfferLogin

[UninstallRun]
; She may be running: stop her (his own processes only: no admin).
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM synaps-avatar.exe"; Flags: runhidden; RunOnceId: "StopHer"

[UninstallDelete]
; What she and the steps wrote: her log and state, her settings, the steps' download cache and this
; installer's note of what it did. Any pack he put in packs\ himself stays (and the one moved aside);
; her pack goes in code (when the steps made it). Installed by install.ps1 (/BY=install.ps1), her
; settings and its cache are its own to remove (its --uninstall keeps them, --purge doesn't).
Type: files; Name: "{localappdata}\synaps-avatar\synaps-avatar.log"
Type: files; Name: "{localappdata}\synaps-avatar\synaps-avatar.old.log"
Type: files; Name: "{localappdata}\synaps-avatar\state.json"
Type: files; Name: "{localappdata}\synaps-avatar\state.json.tmp"
Type: files; Name: "{localappdata}\synaps-avatar\sessions.json"
Type: files; Name: "{localappdata}\synaps-avatar\sessions.json.tmp"
Type: filesandordirs; Name: "{localappdata}\synaps-avatar\installer"; Check: not ByInstallPs1
Type: files; Name: "{app}\capri-syn-setup.ini"
Type: files; Name: "{userappdata}\synaps-avatar\settings.toml"; Check: not ByInstallPs1
Type: files; Name: "{userappdata}\synaps-avatar\settings.toml.tmp"; Check: not ByInstallPs1
Type: dirifempty; Name: "{userappdata}\synaps-avatar"
Type: dirifempty; Name: "{app}\steps"
Type: dirifempty; Name: "{localappdata}\synaps-avatar\packs"
Type: dirifempty; Name: "{localappdata}\synaps-avatar"

[Code]
const
  PackDownload = 0;
  PackFolder = 1;
  PackSkip = 2;

var
  BrainPage, PackPage: TWizardPage;
  SynapsBox: TNewCheckBox;
  SynapsStatus: TNewStaticText;
  DownloadRadio, FolderRadio, SkipRadio: TNewRadioButton;
  FolderEdit: TNewEdit;
  BrowseButton: TNewButton;
  Progress: TOutputMarqueeProgressWizardPage;
  { synaps-check before the pages: '' (not run), ok, old or missing; and what it said. }
  SynapsFound, SynapsFoundVer: String;
  { One step's run: its name and title, its results (key=value) and its "failed:" reason. }
  StepName, StepTitle, StepFailed: String;
  StepResults: TStringList;
  { What the steps did, for the finish page. }
  SynapsOk, SynapsNew, BrainOk, DaemonOk, PackOk, VoiceOk: Boolean;
  SynapsVer, SynapsPath, SynapsWhy, DaemonWhy, PackWhy, VoiceWhy, PackFrom: String;
  Retry, Notices: TStringList;

{ ---- paths and choices ------------------------------------------------------------------------ }

function PackDest(Param: String): String;
begin
  Result := ExpandConstant('{localappdata}\synaps-avatar\packs\capri');
end;

function PowerShell: String;
begin
  Result := ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe');
end;

function StepsScript: String;
begin
  Result := ExpandConstant('{app}\steps\steps.ps1');
end;

{ The command he can run again in a terminal (PowerShell or cmd). }
function StepCommand(const Args: String): String;
begin
  Result := 'powershell -NoProfile -ExecutionPolicy Bypass -File "' + StepsScript + '" ' + Args;
end;

function PackParam: String;
begin
  Result := Trim(ExpandConstant('{param:PACK|}'));
end;

function SynapsParam: String;
begin
  Result := Lowercase(Trim(ExpandConstant('{param:SYNAPS|install}')));
end;

{ /PACK=: download (the default), skip, or a folder. The pack page starts on it. }
function ParamPackChoice: Integer;
var
  P: String;
begin
  P := Lowercase(PackParam);
  if (P = '') or (P = 'download') then Result := PackDownload
  else if (P = 'skip') or (P = 'none') then Result := PackSkip
  else Result := PackFolder;
end;

{ His pick on the pack page (unattended: /PACK=, as the page started on it). }
function PackChoice: Integer;
begin
  if FolderRadio.Checked then Result := PackFolder
  else if SkipRadio.Checked then Result := PackSkip
  else Result := PackDownload;
end;

function PackFolderChosen: String;
begin
  Result := RemoveBackslashUnlessRoot(Trim(FolderEdit.Text));
end;

{ A pack folder: there, with a sprites.json. '' when it will do. }
function PackError(P: String): String;
begin
  Result := '';
  if P = '' then
    Result := 'Pick the pack folder (the one with sprites.json), or another choice.'
  else if not DirExists(P) then
    Result := 'There is no folder ' + P + '.'
  else if not FileExists(AddBackslash(P) + 'sprites.json') then
    Result := P + ' has no sprites.json: it is not a pack.';
end;

function WantSynaps: Boolean;
begin
  if SynapsParam = 'skip' then Result := False
  else if (BrainPage <> nil) and (SynapsFound <> '') and (SynapsFound <> 'ok') then Result := SynapsBox.Checked
  else Result := True;
end;

function PackReady: Boolean;
begin
  Result := FileExists(PackDest('') + '\sprites.json');
end;

function ByInstallPs1: Boolean;
begin
  Result := Lowercase(ExpandConstant('{param:BY|}')) = 'install.ps1';
end;

function OfferLogin: Boolean;
begin
  Result := SynapsOk and not BrainOk;
end;

function StateIni: String;
begin
  Result := ExpandConstant('{app}\capri-syn-setup.ini');
end;

{ ---- running a step ---------------------------------------------------------------------------- }

{ A result the last step gave (key=value lines in StepResults), '' when it gave none. }
function StepResult(const K: String): String;
var
  I: Integer;
begin
  Result := '';
  for I := StepResults.Count - 1 downto 0 do
    if Copy(StepResults[I], 1, Length(K) + 1) = K + '=' then begin
      Result := Copy(StepResults[I], Length(K) + 2, Length(StepResults[I]));
      exit;
    end;
end;

function IsResultKey(const K: String): Boolean;
begin
  Result := Pos(',' + K + ',', ',synaps,brain,daemon,runvalue,pack,voice,release,was,remove,failed,') > 0;
end;

{ One line of a step: in the log; a result is kept; anything else goes on the progress page. }
procedure StepLine(const S: String; const Error, FirstLine: Boolean);
var
  L, K: String;
  P: Integer;
begin
  L := TrimRight(S);
  if L = '' then exit;
  Log(StepName + ': ' + L);
  if Error then exit;
  P := Pos(': ', L);
  if (P > 1) and (L[1] <> ' ') then begin
    K := Copy(L, 1, P - 1);
    if IsResultKey(K) then begin
      StepResults.Add(K + '=' + Copy(L, P + 2, Length(L)));
      if K = 'failed' then StepFailed := Copy(L, P + 2, Length(L));
      exit;
    end;
  end;
  L := Trim(L);
  if (Copy(L, 1, 2) = '+ ') or (Copy(L, 1, 2) = '! ') or (Copy(L, 1, 2) = '$ ') then L := Copy(L, 3, Length(L));
  if Copy(L, 1, 17) = 'Required Notice: ' then Notices.Add(L);
  if Progress <> nil then Progress.SetText(StepTitle, L);
end;

{ steps.ps1 Args in a PowerShell with no window; True when it says it's done. Its reason in StepFailed. }
function RunStep(const Name, Title, Script, Args: String): Boolean;
var
  Code: Integer;
begin
  StepName := Name;
  StepTitle := Title;
  StepFailed := '';
  StepResults.Clear;
  if Progress <> nil then Progress.SetText(Title, '');
  Log('step ' + Name + ': ' + Args);
  Result := False;
  try
    if not ExecAndLogOutput(PowerShell, '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + Script + '" ' + Args,
        '', SW_HIDE, ewWaitUntilTerminated, Code, @StepLine) then begin
      StepFailed := 'PowerShell didn''t start (' + SysErrorMessage(Code) + ')';
      exit;
    end;
  except
    StepFailed := 'PowerShell didn''t run: ' + GetExceptionMessage;
    exit;
  end;
  Result := Code = 0;
  if not Result and (StepFailed = '') then StepFailed := 'it stopped (exit ' + IntToStr(Code) + '): the setup log has its lines';
  Log('step ' + Name + ': exit ' + IntToStr(Code));
end;

{ "ok 0.12.0 C:\...\synaps.exe": the version and the path. }
procedure ReadSynaps(const R: String);
var
  Rest: String;
  P: Integer;
begin
  Rest := R;
  P := Pos(' ', Rest);
  if P = 0 then exit;
  Rest := Copy(Rest, P + 1, Length(Rest));
  P := Pos(' ', Rest);
  if P = 0 then exit;
  SynapsVer := Copy(Rest, 1, P - 1);
  SynapsPath := Copy(Rest, P + 1, Length(Rest));
end;

{ ---- the "Her brain" page: Synaps ---------------------------------------------------------------- }

{ synaps-check, once, from a copy of the steps in the temporary folder (nothing is installed yet). }
procedure CheckSynaps;
var
  Script: String;
begin
  if SynapsFound <> '' then exit;
  SynapsFound := 'missing';
  try
    ExtractTemporaryFiles('{app}\steps\steps.ps1');
    ExtractTemporaryFiles('{app}\steps\installer\manifest');
  except
    Log('synaps-check: ' + GetExceptionMessage);
    exit;
  end;
  Script := ExpandConstant('{tmp}') + '\{app}\steps\steps.ps1';
  RunStep('synaps-check', 'Looking for Synaps', Script, 'synaps-check');
  if StepResult('synaps') <> '' then begin
    SynapsFound := Copy(StepResult('synaps'), 1, Pos(' ', StepResult('synaps') + ' ') - 1);
    ReadSynaps(StepResult('synaps'));
    SynapsFoundVer := SynapsVer;
  end;
  BrainOk := StepResult('brain') = 'yes';
end;

procedure UpdateBrainPage;
begin
  if SynapsFound = 'old' then
    SynapsStatus.Caption := 'Synaps ' + SynapsFoundVer + ' is on this PC, but she needs a newer one. ' +
      'Setup can install its latest release over it.'
  else
    SynapsStatus.Caption := 'Synaps isn''t on this PC yet. Setup can install its latest release now.';
end;

function NewText(Page: TWizardPage; const Caption: String; Left, Top: Integer): TNewStaticText;
begin
  Result := TNewStaticText.Create(Page);
  Result.Parent := Page.Surface;
  Result.Left := Left;
  Result.Top := Top;
  Result.Width := Page.SurfaceWidth - Left;
  Result.WordWrap := True;
  Result.AutoSize := True;
  Result.Caption := Caption;
end;

procedure CreateBrainPage;
var
  T: TNewStaticText;
begin
  BrainPage := CreateCustomPage(wpSelectTasks, 'Her brain', 'She thinks and talks through Synaps.');
  T := NewText(BrainPage, 'Synaps is an open-source agent runtime (github.com/HaseebKhalid1507/SynapsCLI): ' +
    'she talks to her model through its daemon, which runs in the background.', 0, 0);
  SynapsStatus := NewText(BrainPage, '', 0, T.Top + T.Height + ScaleY(12));
  SynapsBox := TNewCheckBox.Create(BrainPage);
  SynapsBox.Parent := BrainPage.Surface;
  SynapsBox.Top := SynapsStatus.Top + ScaleY(44);
  SynapsBox.Width := BrainPage.SurfaceWidth;
  SynapsBox.Height := ScaleY(20);
  SynapsBox.Caption := 'Install Synaps (her brain, open source)';
  SynapsBox.Checked := True;
  NewText(BrainPage, 'With its own installer, from its latest GitHub release, checked against GitHub''s SHA-256. ' +
    'It goes in %USERPROFILE%\.cargo\bin and onto your Path. It''s yours to keep: removing her asks first.',
    ScaleX(18), SynapsBox.Top + SynapsBox.Height + ScaleY(4));
end;

{ ---- the "Her pack" page ------------------------------------------------------------------------- }

procedure PackChoiceClick(Sender: TObject);
begin
  FolderEdit.Enabled := FolderRadio.Checked;
  BrowseButton.Enabled := FolderRadio.Checked;
end;

procedure BrowseClick(Sender: TObject);
var
  Dir: String;
begin
  Dir := FolderEdit.Text;
  if BrowseForFolder('Her pack folder (the one with sprites.json):', Dir, False) then
    FolderEdit.Text := Dir;
end;

function NewRadio(const Caption: String; Top: Integer): TNewRadioButton;
begin
  Result := TNewRadioButton.Create(PackPage);
  Result.Parent := PackPage.Surface;
  Result.Top := Top;
  Result.Width := PackPage.SurfaceWidth;
  Result.Height := ScaleY(20);
  Result.Caption := Caption;
  Result.OnClick := @PackChoiceClick;
end;

procedure CreatePackPage;
var
  T: TNewStaticText;
  Y: Integer;
begin
  PackPage := CreateCustomPage(BrainPage.ID, 'Her pack', 'Which art should she wear?');
  T := NewText(PackPage, 'Her art isn''t in this installer. Setup can download it from its artists'' GitHub ' +
    'repositories and build her pack on this PC.', 0, 0);
  Y := T.Top + T.Height + ScaleY(10);
  DownloadRadio := NewRadio('Download her art (CapriCore + lachy''s desktop art, ~100 MB)', Y);
  T := NewText(PackPage, 'At pinned versions, every file checked against its SHA-256. lachy''s desktop art is under ' +
    'the PolyForm Noncommercial License 1.0.0 (noncommercial use only): its notices are shown when it''s done.',
    ScaleX(18), DownloadRadio.Top + DownloadRadio.Height + ScaleY(2));
  Y := T.Top + T.Height + ScaleY(10);
  FolderRadio := NewRadio('Use a pack folder I already have', Y);
  FolderEdit := TNewEdit.Create(PackPage);
  FolderEdit.Parent := PackPage.Surface;
  FolderEdit.Left := ScaleX(18);
  FolderEdit.Top := FolderRadio.Top + FolderRadio.Height + ScaleY(4);
  FolderEdit.Width := PackPage.SurfaceWidth - ScaleX(18) - ScaleX(100);
  BrowseButton := TNewButton.Create(PackPage);
  BrowseButton.Parent := PackPage.Surface;
  BrowseButton.Caption := 'Browse...';
  BrowseButton.Left := FolderEdit.Left + FolderEdit.Width + ScaleX(10);
  BrowseButton.Top := FolderEdit.Top - ScaleY(1);
  BrowseButton.Width := ScaleX(90);
  BrowseButton.Height := WizardForm.NextButton.Height;
  BrowseButton.OnClick := @BrowseClick;
  Y := FolderEdit.Top + FolderEdit.Height + ScaleY(12);
  SkipRadio := NewRadio('Skip, I''ll add one later', Y);
  NewText(PackPage, 'She needs a pack to show up: it goes in ' + PackDest('') + '.', ScaleX(18),
    SkipRadio.Top + SkipRadio.Height + ScaleY(2));
  case ParamPackChoice of
    PackFolder: begin FolderRadio.Checked := True; FolderEdit.Text := PackParam; end;
    PackSkip: SkipRadio.Checked := True;
  else
    DownloadRadio.Checked := True;
  end;
  PackChoiceClick(nil);
end;

procedure InitializeWizard;
begin
  StepResults := TStringList.Create;
  Retry := TStringList.Create;
  Notices := TStringList.Create;
  CreateBrainPage;
  CreatePackPage;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := False;
  if PageID = BrainPage.ID then begin
    if SynapsParam = 'skip' then begin Result := True; exit; end;
    CheckSynaps;
    UpdateBrainPage;
    { Here and new enough: nothing to ask. }
    Result := SynapsFound = 'ok';
  end;
end;

{ Silent installs "click" Next too: a False here then ends Setup. The box is suppressible
  (/SUPPRESSMSGBOXES), or an unattended install would wait on it for ever. }
function NextButtonClick(CurPageID: Integer): Boolean;
var
  E: String;
begin
  Result := True;
  if (CurPageID = PackPage.ID) and (PackChoice = PackFolder) then begin
    E := PackError(PackFolderChosen);
    if E <> '' then begin
      Log('pack: ' + E);
      SuppressibleMsgBox(E, mbError, MB_OK, IDOK);
      Result := False;
    end;
  end;
end;

{ The same check before anything is installed, whatever the page did. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if PackChoice = PackFolder then Result := PackError(PackFolderChosen);
end;

{ ---- after her files: the steps ----------------------------------------------------------------- }

procedure RunSteps;
begin
  Progress := CreateOutputMarqueeProgressPage('Setting her up', 'Her brain, her art and her daemon. This takes a minute or two; nothing here needs you.');
  Progress.Show;
  try
    if WantSynaps then begin
      SynapsOk := RunStep('synaps', 'Synaps, her brain', StepsScript, 'synaps');
      if SynapsOk then begin
        ReadSynaps(StepResult('synaps'));
        BrainOk := StepResult('brain') = 'yes';
        { Installed where there was none: hers to offer to remove with her. }
        if StepResult('was') = 'missing' then begin
          SynapsNew := True;
          SetIniString('synaps', 'installed', SynapsPath, StateIni);
        end;
      end else begin
        SynapsWhy := StepFailed;
        Retry.Add('Synaps and its daemon: ' + StepCommand('synaps daemon'));
      end;
    end;
    if not WantSynaps and (SynapsParam <> 'skip') and ((SynapsFound = 'old') or (SynapsFound = 'missing')) then
      Retry.Add('Synaps and its daemon: ' + StepCommand('synaps daemon'));
    case PackChoice of
      PackDownload: begin
        PackOk := RunStep('pack', 'Her art: downloading and building her pack', StepsScript, 'pack');
        if not PackOk then begin
          PackWhy := StepFailed;
          Retry.Add('Her art: ' + StepCommand('pack'));
        end;
      end;
      PackFolder: begin
        PackFrom := PackFolderChosen;
        PackOk := RunStep('pack', 'Her pack: copying it into place', StepsScript, 'pack -From "' + PackFrom + '"');
        if not PackOk then begin
          PackWhy := StepFailed;
          Retry.Add('Her pack: ' + StepCommand('pack -From "' + PackFrom + '"'));
        end;
      end;
    end;
    if SynapsOk then begin
      DaemonOk := RunStep('daemon', 'Her brain''s daemon', StepsScript, 'daemon -Synaps "' + SynapsPath + '"');
      DaemonOk := DaemonOk and (Copy(StepResult('daemon'), 1, 7) = 'running');
      if StepResult('brain') <> '' then BrainOk := StepResult('brain') = 'yes';
      if not DaemonOk then begin
        DaemonWhy := StepFailed;
        if DaemonWhy = '' then DaemonWhy := 'it isn''t answering yet';
        Retry.Add('Her brain''s daemon: ' + StepCommand('daemon'));
      end;
    end;
    if WizardIsTaskSelected('voice') then begin
      VoiceOk := RunStep('voice', 'Her voice (~360 MB in all)', StepsScript, 'voice');
      if not VoiceOk then begin
        VoiceWhy := StepFailed;
        Retry.Add('Her voice: ' + StepCommand('voice'));
      end;
    end;
  finally
    Progress.Hide;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then RunSteps;
end;

{ ---- the finish page ----------------------------------------------------------------------------- }

function FinishText: String;
var
  N: String;
begin
  N := #13#10;
  Result := 'She''s installed.' + N + N;
  if SynapsOk then begin
    if SynapsNew then Result := Result + '+ Synaps ' + SynapsVer + ', her brain: installed.' + N
    else Result := Result + '+ Synaps ' + SynapsVer + ', her brain.' + N;
  end else if SynapsWhy <> '' then
    Result := Result + '- Synaps didn''t install: ' + SynapsWhy + N
  else if (SynapsParam <> 'skip') and (SynapsFound = 'old') then
    Result := Result + '- Synaps ' + SynapsFoundVer + ' is older than she needs: she can''t talk until it''s updated.' + N
  else if SynapsParam <> 'skip' then
    Result := Result + '- No Synaps: she can''t talk until it''s installed (github.com/HaseebKhalid1507/SynapsCLI).' + N;
  if DaemonOk then Result := Result + '+ Its daemon runs, and starts when you sign in.' + N
  else if DaemonWhy <> '' then Result := Result + '- Its daemon didn''t start: ' + DaemonWhy + N;
  if PackOk and (PackFrom <> '') then Result := Result + '+ Her pack: a copy of ' + PackFrom + '.' + N
  else if PackOk then Result := Result + '+ Her art: downloaded, built and checked.' + N
  else if PackWhy <> '' then Result := Result + '- Her art didn''t come: ' + PackWhy + ' She''s installed without it.' + N
  else Result := Result + '- No art yet: she shows up once a pack is in ' + PackDest('') + '.' + N;
  if VoiceOk then Result := Result + '+ Her voice, with its models.' + N
  else if VoiceWhy <> '' then Result := Result + '- Her voice didn''t install: ' + VoiceWhy + N;
  Result := Result + N;
  if OfferLogin then
    Result := Result + 'Next, give her a brain: leave "Sign in to Claude" ticked, and sign in in your browser. ' +
      'Or say hi to her: she tells you how.' + N;
  Result := Result + 'Talk to her: Win+Alt+C, or click her.';
end;

function FinishDetails: String;
var
  I: Integer;
begin
  Result := '';
  if Retry.Count > 0 then begin
    Result := 'To try again, run this in PowerShell or a Command Prompt:' + #13#10;
    for I := 0 to Retry.Count - 1 do Result := Result + Retry[I] + #13#10;
  end;
  if Notices.Count > 0 then begin
    if Result <> '' then Result := Result + #13#10;
    Result := Result + 'lachy''s desktop art (PolyForm Noncommercial 1.0.0, https://polyformproject.org/licenses/noncommercial/1.0.0):' + #13#10;
    for I := 0 to Notices.Count - 1 do Result := Result + Notices[I] + #13#10;
  end;
end;

procedure CurPageChanged(CurPageID: Integer);
var
  Y, Bottom: Integer;
  Memo: TNewMemo;
  D: String;
begin
  if CurPageID <> wpFinished then exit;
  WizardForm.FinishedLabel.Caption := FinishText;
  WizardForm.FinishedLabel.AdjustHeight;
  Y := WizardForm.FinishedLabel.Top + WizardForm.FinishedLabel.Height + ScaleY(8);
  D := FinishDetails;
  if D <> '' then begin
    Memo := TNewMemo.Create(WizardForm);
    Memo.Parent := WizardForm.FinishedLabel.Parent;
    Memo.Left := WizardForm.FinishedLabel.Left;
    Memo.Top := Y;
    Memo.Width := WizardForm.FinishedLabel.Width;
    Memo.Height := ScaleY(88);
    Memo.ReadOnly := True;
    Memo.ScrollBars := ssVertical;
    Memo.WordWrap := True;
    Memo.Lines.Text := D;
    Y := Y + Memo.Height + ScaleY(8);
  end;
  Bottom := WizardForm.RunList.Top + WizardForm.RunList.Height;
  if Bottom - Y < ScaleY(44) then Bottom := Y + ScaleY(44);
  WizardForm.RunList.Top := Y;
  WizardForm.RunList.Height := Bottom - Y;
end;

procedure DeinitializeSetup;
begin
  { Unattended there's no finish page: the log has the same. }
  if (StepResults <> nil) and WizardSilent then begin
    Log('finished: ' + FinishText);
    if FinishDetails <> '' then Log('finished: ' + FinishDetails);
  end;
end;

{ ---- uninstall ----------------------------------------------------------------------------------- }

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
  Synaps: String;
begin
  if CurUninstallStep <> usUninstall then exit;
  { She may be running: stop her before her files go. }
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM synaps-avatar.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  { What the steps made: her pack (when they did), their Run\synaps-daemon, her voice. }
  if FileExists(StepsScript) then begin
    Exec(PowerShell, '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + StepsScript + '" remove', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Log('remove: exit ' + IntToStr(Code));
  end;
  { Synaps, when this installed it (there was none): his to keep unless he says so. Unattended: kept. }
  Synaps := GetIniString('synaps', 'installed', '', StateIni);
  if (Synaps <> '') and FileExists(Synaps) and FileExists(StepsScript) and not UninstallSilent then
    if MsgBox('Remove Synaps too?' + #13#10#13#10 + 'Her installer installed it (' + Synaps + '). Other programs may use it. ' +
        'Your Synaps login and sessions stay either way.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      Exec(PowerShell, '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + StepsScript + '" synaps-remove', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;
