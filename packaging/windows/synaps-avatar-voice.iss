; synaps-avatar's VOICE installer (Inno Setup 6): the voice sidecar's bundle, beside her. Per user, no
; admin. A separate download, so her own installer stays small (~9 MB; this one is ~45 MB). Built by
; build-voice.ps1 (CI: the `windows-voice-installer` job, an artifact); docs/WINDOWS.md says what it
; does.
;
; It installs the bundle (packaging/windows/voice-bundle.ps1: CPython 3.13 embeddable + wheels, the
; sidecar, the model fetch) into "voice" in her folder, where she looks for it: she starts it while
; [voice] enabled is on. The models are never in here: the "models" task (checked) runs
; voice-models.ps1, which fetches them (~320 MB, SHA-256 pinned) into %LOCALAPPDATA%\capri-voice\models.
;
; Unattended: setup.exe /VERYSILENT /SUPPRESSMSGBOXES [/MERGETASKS="!models"]

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Build
  #define Build "local"
#endif
#ifndef Bundle
  #define Bundle "build\voice"
#endif
; Her own installer's AppId (synaps-avatar.iss): where she is installed.
#define HerAppId "{2147942F-E90C-4536-BEA2-8AB4B7F51734}"

[Setup]
AppId={{B7E0CA8F-064C-4B7F-939A-41681146411C}
AppName=synaps-avatar voice
AppVersion={#AppVersion}
AppVerName=synaps-avatar voice {#AppVersion} ({#Build})
AppPublisher=Haseeb Khalid
AppPublisherURL=https://github.com/HaseebKhalid1507/capri-syn
PrivilegesRequired=lowest
; Beside her (her own install folder if she is installed, else where her installer puts her).
DefaultDirName={code:VoiceDir}
DisableDirPage=yes
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir=Output
OutputBaseFilename=synaps-avatar-voice-setup-{#AppVersion}-{#Build}
UninstallDisplayName=synaps-avatar voice
; A running sidecar holds python.exe: an upgrade closes it (she starts it again after).
CloseApplications=force
RestartApplications=no

[Tasks]
Name: models; Description: "Download her voice models now (~320 MB: Whisper, Kokoro, Silero, the wake word; SHA-256 checked)"

[Files]
Source: "{#Bundle}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
; Again later (a cut download goes on where it stopped).
Name: "{autoprograms}\synaps-avatar voice models"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -NoExit -File ""{app}\voice-models.ps1"""; WorkingDir: "{app}"; Comment: "Fetch (or finish fetching) her voice models"

[Run]
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\voice-models.ps1"""; WorkingDir: "{app}"; StatusMsg: "Fetching her voice models (~320 MB)..."; Flags: waituntilterminated; Tasks: models

[UninstallRun]
; The sidecar, if she runs it now: only the python.exe in this folder (his other Pythons are his).
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""Get-Process python -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -like '{app}\*' } | Stop-Process -Force"""; Flags: runhidden; RunOnceId: "StopSidecar"

[UninstallDelete]
; What runs wrote: Python's caches in the bundle; the models, its log, its link and media state.
Type: filesandordirs; Name: "{app}\python"
Type: filesandordirs; Name: "{app}\site-packages"
Type: filesandordirs; Name: "{app}\__pycache__"
Type: dirifempty; Name: "{app}"
Type: filesandordirs; Name: "{localappdata}\capri-voice"

[Code]
{ Her install folder from her own installer's record, else its default; voice goes inside it. }
function VoiceDir(Param: String): String;
var
  Her: String;
begin
  if not RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#HerAppId}_is1', 'InstallLocation', Her) then
    Her := ExpandConstant('{autopf}\synaps-avatar');
  Result := AddBackslash(RemoveBackslashUnlessRoot(Her)) + 'voice';
end;
