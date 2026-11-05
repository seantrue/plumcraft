; The Windows installer, compiled by packaging/windows.sh with Inno Setup 6:
;
;   ISCC.exe /DVersion=X.Y.Z /DStage=<staged folder> /DOutputDir=<dist> packaging\plumcraft.iss
;
; It installs for this user by default (no administrator needed), or for everyone if chosen.
; The editor goes on the Start menu, and optionally the desktop; the folder can be added to the
; user's PATH, for the `plumcraft` command line. The uninstaller takes all of it back.

#ifndef Version
  #error Pass /DVersion=X.Y.Z
#endif
#ifndef Stage
  #error Pass /DStage=<the staged folder>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
; Never change AppId: it is how an upgrade finds the installed copy.
AppId={{E60F1AC0-9553-42AA-9CD7-0E86FD338764}
AppName=PlumCraft
AppVersion={#Version}
AppVerName=PlumCraft {#Version}
AppPublisher=Sean D. True
AppPublisherURL=https://github.com/seantrue/plumcraft
AppSupportURL=https://github.com/seantrue/plumcraft/issues
DefaultDirName={autopf}\PlumCraft
DefaultGroupName=PlumCraft
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesEnvironment=yes
OutputDir={#OutputDir}
OutputBaseFilename=PlumCraft-{#Version}-windows-x86_64-setup
SetupIconFile={#Stage}\plumcraft.ico
UninstallDisplayIcon={app}\plumcraft-editor.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: desktopicon; Description: "Put PlumCraft on the desktop"; Flags: unchecked
Name: addtopath; Description: "Add PlumCraft to PATH, for the plumcraft command line"; Flags: unchecked

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\PlumCraft"; Filename: "{app}\plumcraft-editor.exe"; IconFilename: "{app}\plumcraft.ico"
Name: "{autodesktop}\PlumCraft"; Filename: "{app}\plumcraft-editor.exe"; IconFilename: "{app}\plumcraft.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\plumcraft-editor.exe"; Description: "Open PlumCraft"; Flags: nowait postinstall skipifsilent

[Code]
// PATH is the user's (HKCU) for a per-user install and the machine's for an all-users one.
function EnvRoot: Integer;
begin
  if IsAdminInstallMode then Result := HKEY_LOCAL_MACHINE else Result := HKEY_CURRENT_USER;
end;

function EnvKey: String;
begin
  if IsAdminInstallMode then
    Result := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
  else
    Result := 'Environment';
end;

// Whether `dir` is one of the ;-separated entries of `path`, ignoring case and a trailing \.
function HasEntry(const path, dir: String): Boolean;
var
  p, d: String;
begin
  p := ';' + Lowercase(path) + ';';
  d := Lowercase(RemoveBackslashUnlessRoot(dir));
  Result := (Pos(';' + d + ';', p) > 0) or (Pos(';' + d + '\;', p) > 0);
end;

procedure AddToPath(const dir: String);
var
  path: String;
begin
  if not RegQueryStringValue(EnvRoot, EnvKey, 'Path', path) then path := '';
  if HasEntry(path, dir) then exit;
  if (path <> '') and (path[Length(path)] <> ';') then path := path + ';';
  RegWriteExpandStringValue(EnvRoot, EnvKey, 'Path', path + dir);
end;

procedure RemoveFromPath(const dir: String);
var
  path, d: String;
  i: Integer;
begin
  if not RegQueryStringValue(EnvRoot, EnvKey, 'Path', path) then exit;
  d := RemoveBackslashUnlessRoot(dir);
  path := ';' + path + ';';
  i := Pos(';' + Lowercase(d) + ';', Lowercase(path));
  if i = 0 then exit;
  Delete(path, i, Length(d) + 1);
  path := Copy(path, 2, Length(path) - 2);
  RegWriteExpandStringValue(EnvRoot, EnvKey, 'Path', path);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
