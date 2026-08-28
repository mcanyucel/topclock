; Inno Setup script for topclock.
;
; Build the app first, then compile this:
;
;     cargo build --release
;     iscc installer\topclock.iss
;
; The installer lands in dist\.

#define AppName      "topclock"
; Overridable so CI can pass the release version: iscc /DAppVersion=1.2.3 ...
#ifndef AppVersion
  #define AppVersion "0.4.1"
#endif
#define AppPublisher "Mustafa Can Yucel"
#define AppExeName   "topclock.exe"

[Setup]
; Never change AppId: it is what lets a new version find and upgrade the old
; one instead of installing alongside it.
AppId={{8B6D9EC4-8537-46A5-93FF-EECC2BD8E71E}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
VersionInfoVersion={#AppVersion}

; Per-user by default. {autopf} resolves to %LocalAppData%\Programs when the
; installer runs unelevated, and to Program Files if the user chooses an
; all-users install in the privileges dialog. The app supports both: it keeps
; its ini beside the exe where that is writable, and falls back to
; %APPDATA%\topclock otherwise, so neither layout needs elevation to configure.
DefaultDirName={autopf}\{#AppName}
PrivilegesRequired=lowest
; "commandline", not "dialog": a dialog here would ask every user to choose
; between an all-users and a just-me install before the wizard even starts, and
; it is shown even under /VERYSILENT unless the command line already answers it.
; Installing per-user is the right default for a personal utility, so it happens
; without asking. Machine-wide deployment still works by passing /ALLUSERS,
; which elevates and installs into Program Files.
PrivilegesRequiredOverridesAllowed=commandline

DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename={#AppName}-{#AppVersion}-setup
SetupIconFile=..\assets\topclock.ico
UninstallDisplayIcon={app}\{#AppExeName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes

; The binary is built for x86_64-pc-windows-msvc.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=6.1sp1

; topclock is likely to be running during an upgrade, and it holds a lock on
; its own exe. Restart Manager closes it first: the window has no special
; WM_CLOSE handling, so DefWindowProc tears it down cleanly.
CloseApplications=yes
CloseApplicationsFilter=*.exe

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startup"; Description: "Start {#AppName} when I sign in"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

; No topclock.ini is shipped, deliberately. A file beside the exe wins the
; app's config lookup, so installing one into Program Files would hand the user
; settings they cannot edit without elevation, and their per-user file would
; never be read. The first run creates one in the right place on its own.

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
; Locating the ini by hand is the fiddly part of configuring this, so put it in
; the Start Menu next to the app. Only for per-user installs: an all-users
; install keeps each user's ini under their own %APPDATA%, which one shared
; shortcut cannot point at.
Name: "{autoprograms}\{#AppName} settings"; Filename: "{app}\topclock.ini"; \
    Check: not IsAdminInstallMode
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon
Name: "{autostartup}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: startup

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; \
    Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The ini the app writes beside itself when {app} is writable, which is the
; usual case for a per-user install.
Type: files; Name: "{app}\topclock.ini"

[Code]
{ Settings in %APPDATA% are user data, so they outlive the uninstall unless the
  user says otherwise. A silent uninstall never prompts and always keeps them. }
procedure CurUninstallStepChanged(CurStep: TUninstallStep);
var
  Dir: String;
begin
  if CurStep <> usPostUninstall then
    Exit;

  Dir := ExpandConstant('{userappdata}\topclock');
  if not DirExists(Dir) or UninstallSilent then
    Exit;

  if MsgBox('Remove your topclock settings as well?' + #13#10#13#10 + Dir,
            mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
    DelTree(Dir, True, True, True);
end;
