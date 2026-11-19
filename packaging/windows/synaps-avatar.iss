; capri-syn's Windows installer (Inno Setup 6): everything she needs, per user, no admin. Built by
; build.ps1 (CI: the `windows-installer` job, an artifact); docs/WINDOWS.md says what it does.
;
; Her binary (synaps-avatar.exe, the windowed build), her shortcut and toast name, then, after the files,
; the install steps install.ps1 runs too: one copy of them, packaging/windows/steps.ps1, installed in
; {app}\steps with what they read (the manifest, make-pack, the pins, her moods and persona: no art).
;   - Synaps, her brain: found as install.ps1 finds it; if it's missing or too old, the "Her brain"
;     page installs the Synaps she's pinned to (installer/manifest: synaps_pin, a release she was run
;     against; never the latest) with its own installer, checked against its pinned SHA-256.
;   - Her pack: the "Her pack" page downloads her art and builds it on his machine (the default), copies
;     a pack folder he has, or skips. Her art is never in this installer.
;   - The Synaps daemon, now and at sign-in (no console window).
;   - Her voice (page "Her voice", on when Windows has a microphone; never by default on an upgrade:
;     one that is installed keeps his settings): the voice installer and its models, downloaded; his
;     choices (microphone, "hey Capri", replies aloud) saved by her own settings writer. Never her
;     voice itself: she keeps her built-in default.
; A step that fails doesn't fail Setup: she is installed, and the finish page says what happened in
; plain words, with the one command that tries it again. Unattended, the log says it.
;
; Unattended: setup.exe /VERYSILENT /SUPPRESSMSGBOXES [/SYNAPS=install|skip]
;             [/PACK=download|skip|"C:\packs\capri"] [/VOICE=on|off] [/MIC="Microphone (...)"]
;             [/VOICEFROM="C:\...\capri-syn-voice-setup.exe" or a URL] [/TASKS=autostart]
; The defaults are the wizard's: Synaps if it's needed, her art downloaded, autostart on, her voice
; when Windows has a microphone (on a first install only). Her voice comes from /VOICEFROM (a file,
; or an https URL), else the latest release; never from a file found by name beside this setup (that
; folder is usually Downloads). With neither, the page says it comes with her first release, and
; stays off. Run as administrator, Setup asks first (she and her daemon would run elevated).
; install.ps1 runs it with /SYNAPS=skip /PACK=skip /VOICE=off /BY=install.ps1: those are its own
; steps (the same steps.ps1), and her settings and its cache are its own to remove.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Build
  #define Build "local"
#endif
#ifndef Exe
  #define Exe "..\..\target\release\synaps-avatar.exe"
#endif
; Her icon: the master in assets\icon (assets/icon/NOTICE), used as is.
#define IconDir "..\..\assets\icon"
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
; Hers: black (Inno 6.6's dark style), her icon the red in it. An older Inno: its light one.
#if VER >= EncodeVer(6,6,0)
WizardStyle=modern dark
#else
WizardStyle=modern
#endif
Compression=lzma2/max
SolidCompression=yes
OutputDir=Output
OutputBaseFilename=capri-syn-setup-{#AppVersion}-{#Build}
SetupIconFile={#IconDir}\capri-syn.ico
#if VER >= EncodeVer(6,5,2)
WizardSmallImageFile={#IconDir}\capri-syn-256.png
#endif
UninstallDisplayIcon={app}\synaps-avatar.ico
UninstallDisplayName=capri-syn
; An upgrade closes the running one (Restart Manager), and doesn't restart it.
CloseApplications=force
RestartApplications=no

[Tasks]
Name: autostart; Description: "Start her when I sign in"

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
Source: "{#IconDir}\capri-syn.ico"; DestDir: "{app}"; DestName: "synaps-avatar.ico"; Flags: ignoreversion
Source: "{#IconDir}\capri-syn-256.png"; DestDir: "{app}"; DestName: "synaps-avatar.png"; Flags: ignoreversion

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

[UninstallDelete]
; Her old exe, renamed aside by an upgrade while a process of hers still mapped it.
Type: files; Name: "{app}\synaps-avatar.exe.old-*"
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
  BrainPage, PackPage, VoicePage: TWizardPage;
  TalkBox, WakeBox, ReadBox: TNewCheckBox;
  TalkNote, MicLabel: TNewStaticText;
  MicCombo: TNewComboBox;
  { The microphones Windows has (steps.ps1 mics), once; -1: not looked yet. }
  MicCount: Integer;
  { Where her voice would come from (steps.ps1 voice-check): file, url, release, none or offline. }
  VoiceKind: String;
  SynapsBox: TNewCheckBox;
  SynapsStatus: TNewStaticText;
  DownloadRadio, FolderRadio, SkipRadio: TNewRadioButton;
  FolderEdit: TNewEdit;
  BrowseButton: TNewButton;
  Progress: TOutputMarqueeProgressWizardPage;
  { synaps-check before the pages: '' (not run), ok, old or missing; and what it said. }
  SynapsFound, SynapsFoundVer: String;
  { The Synaps Setup installs: installer/manifest's synaps_pin (synaps-check says it). }
  SynapsPin: String;
  { Her install before this one (an upgrade: her AppId's uninstall key), and her voice's: read at
    the start, before this install writes its own. }
  HerBefore, VoiceBefore: Boolean;
  { One step's run: its name and title, its results (key=value) and its "failed:" reason. }
  StepName, StepTitle, StepFailed: String;
  StepResults: TStringList;
  { What the steps did, for the finish page. }
  SynapsOk, SynapsNew, SynapsPut, BrainOk, DaemonOk, DaemonWrong, PackOk, VoiceOk, VoiceRan: Boolean;
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
  Result := Pos(',' + K + ',', ',synaps,brain,daemon,runvalue,pack,voice,voicefrom,release,was,remove,mic,mics,failed,pin,') > 0;
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
  SynapsPin := StepResult('pin');
end;

procedure UpdateBrainPage;
var
  Which: String;
begin
  if SynapsPin <> '' then Which := 'Synaps ' + SynapsPin + ', the one she was tested with'
  else Which := 'the Synaps she was tested with';
  if SynapsFound = 'old' then
    SynapsStatus.Caption := 'Synaps ' + SynapsFoundVer + ' is on this PC, but she needs a newer one. ' +
      'Setup can install ' + Which + ', over it.'
  else
    SynapsStatus.Caption := 'Synaps isn''t on this PC yet. Setup can install ' + Which + ', now.';
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
  NewText(BrainPage, 'With its own installer, from the GitHub release she was tested with, checked against its pinned SHA-256. ' +
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

{ ---- the "Her voice" page ------------------------------------------------------------------------ }

function VoiceParam: String;
begin
  Result := Lowercase(Trim(ExpandConstant('{param:VOICE|}')));
end;

procedure VoiceClick(Sender: TObject);
begin
  MicLabel.Enabled := TalkBox.Checked;
  MicCombo.Enabled := TalkBox.Checked;
  WakeBox.Enabled := TalkBox.Checked;
  ReadBox.Enabled := TalkBox.Checked;
end;

{ Her voice installer: /VOICEFROM, else '' (the release). Never a file found by name beside Setup:
  that folder is usually Downloads. }
function VoiceFrom: String;
begin
  Result := Trim(ExpandConstant('{param:VOICEFROM|}'));
end;

{ steps.ps1 mics and voice-check, once (from the temporary copy): the picker's names, where her voice
  would come from, and the page's default. }
procedure CheckMics;
var
  I, Pick: Integer;
  M, A: String;
begin
  if MicCount >= 0 then exit;
  MicCount := 0;
  { Off (install.ps1 runs it so: its --voice is its own): no need to look. }
  if VoiceParam = 'off' then begin
    TalkBox.Checked := False;
    VoiceClick(nil);
    exit;
  end;
  VoiceKind := 'none';
  try
    ExtractTemporaryFiles('{app}\steps\steps.ps1');
    ExtractTemporaryFiles('{app}\steps\installer\manifest');
    { An empty "" argument can be lost on its way to -File: none is passed. }
    A := 'mics voice-check';
    if VoiceFrom <> '' then A := A + ' -VoiceFrom "' + VoiceFrom + '"';
    RunStep('mics', 'Looking for a microphone', ExpandConstant('{tmp}') + '\{app}\steps\steps.ps1', A);
  except
    Log('mics: ' + GetExceptionMessage);
  end;
  for I := 0 to StepResults.Count - 1 do
    if Copy(StepResults[I], 1, 4) = 'mic=' then begin
      MicCombo.Items.Add(Copy(StepResults[I], 5, Length(StepResults[I])));
      MicCount := MicCount + 1;
    end;
  if StepResult('voicefrom') <> '' then
    VoiceKind := Copy(StepResult('voicefrom'), 1, Pos(' ', StepResult('voicefrom') + ' ') - 1);
  { /MIC=: that one (kept as given when Windows doesn't list it: she says so if it's gone). }
  M := Trim(ExpandConstant('{param:MIC|}'));
  Pick := 0;
  if M <> '' then begin
    Pick := MicCombo.Items.IndexOf(M);
    if Pick < 0 then Pick := MicCombo.Items.Add(M);
  end;
  MicCombo.ItemIndex := Pick;
  { An upgrade never turns her voice on by itself: one installed keeps his settings (microphone,
    "hey Capri", replies aloud), one he left off stays off. Only his tick or /VOICE=on sets it up. }
  if VoiceParam = 'on' then TalkBox.Checked := True
  else if VoiceParam = 'off' then TalkBox.Checked := False
  else if HerBefore or VoiceBefore then TalkBox.Checked := False
  else TalkBox.Checked := MicCount > 0;
  if (VoiceParam = '') and VoiceBefore then
    TalkNote.Caption := 'Her voice is installed already, with your settings. Tick this only to choose them again.';
  if MicCount = 0 then begin
    TalkNote.Caption := 'No microphone found. Plug one in and run Setup again to talk to her.';
    TalkBox.Enabled := VoiceParam = 'on';
  end;
  { Nothing to install it from: off and greyed, whatever was asked. }
  if VoiceKind = 'none' then begin
    TalkNote.Caption := 'Her voice comes with her first release. Run Setup again then to talk to her.';
    TalkBox.Checked := False;
    TalkBox.Enabled := False;
  end else if VoiceKind = 'offline' then begin
    TalkNote.Caption := 'Can''t reach GitHub for her voice. Run Setup again when you''re online to talk to her.';
    TalkBox.Checked := False;
    TalkBox.Enabled := False;
  end;
  VoiceClick(nil);
end;

function WantVoice: Boolean;
begin
  Result := (VoicePage <> nil) and TalkBox.Checked;
end;

{ His microphone: '' for Windows' default. }
function MicChosen: String;
begin
  Result := '';
  if MicCombo.ItemIndex > 0 then Result := MicCombo.Items[MicCombo.ItemIndex];
end;

function OnOff(B: Boolean): String;
begin
  if B then Result := 'on' else Result := 'off';
end;

{ steps.ps1 voice's arguments: his choices (all of them: he ticked the box), and where his voice comes
  from when it isn't the release. }
function VoiceArgs: String;
var
  V: String;
begin
  Result := 'voice -WakeWord ' + OnOff(WakeBox.Checked) + ' -ReadAloud ' + OnOff(ReadBox.Checked);
  if MicChosen <> '' then Result := Result + ' -Mic "' + MicChosen + '"'
  else Result := Result + ' -MicDefault';
  V := VoiceFrom;
  if V <> '' then Result := Result + ' -VoiceFrom "' + V + '"';
end;

function NewBox(Page: TWizardPage; const Caption: String; Top: Integer): TNewCheckBox;
begin
  Result := TNewCheckBox.Create(Page);
  Result.Parent := Page.Surface;
  Result.Top := Top;
  Result.Width := Page.SurfaceWidth;
  Result.Height := ScaleY(20);
  Result.Caption := Caption;
end;

procedure CreateVoicePage;
var
  T: TNewStaticText;
begin
  VoicePage := CreateCustomPage(PackPage.ID, 'Her voice', 'Talk to her out loud, and she talks back.');
  TalkBox := NewBox(VoicePage, 'Talk to her', 0);
  TalkBox.OnClick := @VoiceClick;
  TalkNote := NewText(VoicePage, 'Downloads her voice and its speech models, about 420 MB. What you say is turned ' +
    'into words on this PC; only the words go to her brain.', ScaleX(18), TalkBox.Top + TalkBox.Height + ScaleY(2));
  MicLabel := NewText(VoicePage, 'Microphone', ScaleX(18), TalkNote.Top + ScaleY(40));
  MicLabel.AutoSize := True;
  MicCombo := TNewComboBox.Create(VoicePage);
  MicCombo.Parent := VoicePage.Surface;
  MicCombo.Style := csDropDownList;
  MicCombo.Left := ScaleX(110);
  MicCombo.Top := MicLabel.Top - ScaleY(3);
  MicCombo.Width := VoicePage.SurfaceWidth - ScaleX(110);
  MicCombo.Items.Add('Windows default');
  MicCombo.ItemIndex := 0;
  WakeBox := NewBox(VoicePage, '"Hey Capri": she listens for her name', MicCombo.Top + MicCombo.Height + ScaleY(14));
  WakeBox.Left := ScaleX(18);
  WakeBox.Width := VoicePage.SurfaceWidth - ScaleX(18);
  WakeBox.Checked := True;
  T := NewText(VoicePage, 'While it''s on, the microphone stays open for her name. Her tag shows it.',
    ScaleX(36), WakeBox.Top + WakeBox.Height + ScaleY(2));
  ReadBox := NewBox(VoicePage, 'Read replies aloud', T.Top + T.Height + ScaleY(10));
  ReadBox.Left := ScaleX(18);
  ReadBox.Width := VoicePage.SurfaceWidth - ScaleX(18);
  ReadBox.Checked := True;
  NewText(VoicePage, 'Her replies to what comes in on its own (events, other sessions) are spoken too. ' +
    'Her answers to what you say out loud always are.', ScaleX(36), ReadBox.Top + ReadBox.Height + ScaleY(2));
  VoiceClick(nil);
end;

{ Run as administrator (a right-click, or from an elevated prompt), what Setup starts would run so
  too: her, her brain's daemon. Asked first, No the default. Unattended: logged, and on (CI's runners
  and an elevated install.ps1 are elevated; their caller chose that). }
function InitializeSetup: Boolean;
begin
  Result := True;
  if not IsAdmin then exit;
  Log('Setup runs as administrator: she and her brain''s daemon would run elevated too.');
  if WizardSilent then exit;
  Result := MsgBox('Setup is running as administrator.' + #13#10#13#10 +
    'She and her brain''s daemon would then run as administrator too, which they don''t need. ' +
    'Better: close this, and double-click Setup (no "Run as administrator").' + #13#10#13#10 +
    'Go on as administrator anyway?', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
end;

procedure InitializeWizard;
begin
  StepResults := TStringList.Create;
  Retry := TStringList.Create;
  Notices := TStringList.Create;
  MicCount := -1;
  HerBefore := RegKeyExists(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{2147942F-E90C-4536-BEA2-8AB4B7F51734}_is1');
  VoiceBefore := RegKeyExists(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#VoiceAppId}_is1');
  CreateBrainPage;
  CreatePackPage;
  CreateVoicePage;
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
  if PageID = VoicePage.ID then CheckMics;
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
        { Installed or upgraded now: a daemon on another protocol is an older one still running. }
        SynapsPut := (StepResult('was') = 'missing') or (StepResult('was') = 'old');
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
      if SynapsPut then
        DaemonOk := RunStep('daemon', 'Her brain''s daemon', StepsScript, 'daemon -Fresh -Synaps "' + SynapsPath + '"')
      else
        DaemonOk := RunStep('daemon', 'Her brain''s daemon', StepsScript, 'daemon -Synaps "' + SynapsPath + '"');
      DaemonOk := DaemonOk and (Copy(StepResult('daemon'), 1, 7) = 'running');
      DaemonWrong := StepResult('daemon') = 'wrong-protocol';
      if StepResult('brain') <> '' then BrainOk := StepResult('brain') = 'yes';
      if not DaemonOk then begin
        DaemonWhy := StepFailed;
        if DaemonWhy = '' then DaemonWhy := 'it isn''t answering yet';
        Retry.Add('Her brain''s daemon: ' + StepCommand('daemon'));
      end;
    end;
    { Unattended with /VOICE= and no look at the page yet: its default needs the microphones. }
    if MicCount < 0 then CheckMics;
    if WantVoice then begin
      VoiceRan := True;
      VoiceOk := RunStep('voice', 'Her voice: downloading it and its speech models (~420 MB)', StepsScript, VoiceArgs);
      if not VoiceOk then begin
        VoiceWhy := StepFailed;
        Retry.Add('Her voice: ' + StepCommand(VoiceArgs));
      end;
    end else if VoiceKind = 'none' then Log('voice: off (it comes with her first release)')
    else if VoiceKind = 'offline' then Log('voice: off (GitHub unreachable)')
    else if MicCount = 0 then Log('voice: off (no microphone found)')
    else Log('voice: off');
  finally
    Progress.Hide;
  end;
end;

{ ---- her exe, even when a process of hers that won't go maps it ---------------------------------- }

{ Restart Manager closes her first (CloseApplications=force). A process of hers that was killed and
  never went (on his box: one thread, 707 handles, for good) still maps her exe, so it can't be
  replaced ("DeleteFile failed; code 5", and Setup aborted). Renaming a mapped exe works: just
  before her files go in, the old one is renamed aside; deleted once the new one is in (left for
  the next Setup or the uninstall when it's still mapped); renamed back if Setup stops before. }
var
  ExeAside: String;
  FilesIn: Boolean;

function HerExe: String;
begin
  Result := ExpandConstant('{app}\synaps-avatar.exe');
end;

{ What earlier Setups left aside (mapped then, maybe free now). }
procedure DeleteOldExes;
var
  F: TFindRec;
  D: String;
begin
  D := ExpandConstant('{app}\');
  if FindFirst(D + 'synaps-avatar.exe.old-*', F) then begin
    try
      repeat
        if DeleteFile(D + F.Name) then Log('her exe: deleted ' + F.Name + ' (left aside before)')
        else Log('her exe: ' + F.Name + ' is still in use: left');
      until not FindNext(F);
    finally
      FindClose(F);
    end;
  end;
end;

procedure SetExeAside;
begin
  DeleteOldExes;
  if not FileExists(HerExe) then exit;
  ExeAside := HerExe + '.old-' + GetDateTimeString('yyyymmddhhnnss', #0, #0);
  if RenameFile(HerExe, ExeAside) then Log('her exe: the old one renamed aside (' + ExtractFileName(ExeAside) + ')')
  else begin
    Log('her exe: the old one could not be renamed aside: replaced in place');
    ExeAside := '';
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then SetExeAside;
  if CurStep = ssPostInstall then begin
    FilesIn := True;
    if ExeAside <> '' then begin
      if DeleteFile(ExeAside) then Log('her exe: the old one deleted')
      else Log('her exe: the old one is still in use (a process of hers that never went): left for the next Setup or the uninstall');
    end;
    RunSteps;
  end;
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
  else if DaemonWrong then Result := Result + '- Its daemon runs, but she can''t use it: ' + DaemonWhy + N
  else if DaemonWhy <> '' then Result := Result + '- Its daemon didn''t start: ' + DaemonWhy + N;
  if PackOk and (PackFrom <> '') then Result := Result + '+ Her pack: a copy of ' + PackFrom + '.' + N
  else if PackOk then Result := Result + '+ Her art: downloaded, built and checked.' + N
  else if PackWhy <> '' then Result := Result + '- Her art didn''t come: ' + PackWhy + ' She''s installed without it.' + N
  else Result := Result + '- No art yet: she shows up once a pack is in ' + PackDest('') + '.' + N;
  if VoiceOk then begin
    Result := Result + '+ Her voice: say "hey Capri"';
    if not WakeBox.Checked then Result := Result + ', or hold Win+Alt+C,';
    Result := Result + ' and talk.';
    if MicChosen <> '' then Result := Result + ' Microphone: ' + MicChosen + '.';
    Result := Result + N;
  end else if VoiceWhy <> '' then Result := Result + '- Her voice didn''t come: ' + VoiceWhy + ' She''s installed without it.' + N
  else if VoiceBefore and not VoiceRan then Result := Result + '+ Her voice: as you set it.' + N
  else if VoiceKind = 'none' then Result := Result + '- Her voice comes with her first release.' + N
  else if VoiceKind = 'offline' then Result := Result + '- No voice: GitHub couldn''t be reached. Run Setup again to add it.' + N
  else if (VoicePage <> nil) and (MicCount = 0) and (VoiceParam <> 'off') then Result := Result + '- No voice: no microphone found.' + N;
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
  { Setup stopped before her files were in: her old exe back where it was. }
  if (ExeAside <> '') and not FilesIn and not FileExists(HerExe) and FileExists(ExeAside) then
    if RenameFile(ExeAside, HerExe) then Log('her exe: Setup stopped: the old one put back')
    else Log('her exe: Setup stopped, and the old one could not be put back: it is ' + ExeAside);
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
  Synaps, Her: String;
begin
  if CurUninstallStep <> usUninstall then exit;
  { She may be running: stop her before her files go. Only this install of her (by its path), never
    another synaps-avatar.exe (a build of his elsewhere). }
  Her := HerExe;
  StringChangeEx(Her, '''', '''''', True);
  Exec(PowerShell, '-NoProfile -NonInteractive -Command "$e = ''' + Her + '''; ' +
    'Get-Process synaps-avatar -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $e } | ' +
    'ForEach-Object { Stop-Process -Id $_.Id -Force; Wait-Process -Id $_.Id -Timeout 5 -ErrorAction SilentlyContinue }"',
    '', SW_HIDE, ewWaitUntilTerminated, Code);
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
      Exec(PowerShell, '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + StepsScript + '" synaps-remove -Synaps "' + Synaps + '"', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;
