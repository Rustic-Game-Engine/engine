#ifndef AddToPathByDefault
  #define AddToPathByDefault "0"
#endif
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef BuildSuffix
  #define BuildSuffix ""
#endif
#ifndef BuildCommit
  #define BuildCommit "development"
#endif

#define AppName "Rustic Game Engine"
#define AppPublisher "Rustic Game Engine Contributors"
#define AppExeName "rustic-project-manager.exe"

[Setup]
AppId={{2FD23D55-DBA1-470C-8A03-E649615934EA}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\Rustic Game Engine
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=RusticGameEngine-Setup-{#AppVersion}-x64{#BuildSuffix}
SetupIconFile=..\assets\app-icon.ico
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog
UsedUserAreasWarning=no
UninstallDisplayIcon={app}\{#AppExeName}
WizardStyle=modern
MinVersion=10.0
CloseApplications=yes
RestartApplications=no
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
SelectComponentsLabel2=Select the gameplay languages to install. Lua 5.4, Luau, JavaScript/QuickJS, and HTML/CSS are bundled. Optional toolchains require an internet connection and can be added later by rerunning Setup or using Language Toolchain Manager from the Start menu.

[Types]
Name: "recommended"; Description: "Recommended installation"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

[Components]
Name: "engine"; Description: "Rustic Game Engine"; Types: recommended custom; Flags: fixed
Name: "languages"; Description: "Gameplay languages (rerun Setup or use Language Toolchain Manager later to change these)"; Types: recommended custom; Flags: fixed
Name: "languages\bundled"; Description: "Lua 5.4, Luau, JavaScript/QuickJS, and HTML/CSS (bundled)"; Types: recommended custom; Flags: fixed
Name: "languages\python"; Description: "Python - install Python 3.14"; Types: custom
Name: "languages\csharp"; Description: "C# - install .NET SDK 10"; Types: custom
Name: "languages\ccpp"; Description: "C and C++ - install LLVM/Clang"; Types: custom
Name: "languages\java"; Description: "Java - install Microsoft OpenJDK 21"; Types: custom
Name: "languages\php"; Description: "PHP - install PHP 8.4 CLI"; Types: custom

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked
#if AddToPathByDefault == "1"
Name: "addtopath"; Description: "Add the installation folder to PATH"; GroupDescription: "Command-line access:"
#else
Name: "addtopath"; Description: "Add the installation folder to PATH"; GroupDescription: "Command-line access:"; Flags: unchecked
#endif

[Files]
Source: "..\licenses\Luau-LICENSE.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\licenses\Symphonia-MPL-2.0.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\licenses\ogg-BSD-3-Clause.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\licenses\opus-pure-BSD-3-Clause.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\licenses\opus-pure-ATTRIBUTION.md"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\licenses\Audio-Decoders-NOTICE.md"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\assets\icons\lucide\LICENSE"; DestDir: "{app}\licenses"; DestName: "Lucide-LICENSE.txt"; Flags: ignoreversion
Source: "..\assets\icons\material-design\LICENSE"; DestDir: "{app}\licenses"; DestName: "Material-Design-Icons-LICENSE.txt"; Flags: ignoreversion
#ifndef InstallerPreflight
Source: "..\target\distribution\rustic-luau-host.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-agent-backend.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-setup-helper.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-project-manager.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-editor.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-runtime.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-asset-worker.exe"; DestDir: "{app}"; Flags: ignoreversion
#endif
Source: "..\tools\manage-language-toolchains.ps1"; DestDir: "{app}\tools"; Flags: ignoreversion
Source: "..\tools\update-desktop.ps1"; DestDir: "{app}\tools"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"; WorkingDir: "{app}"
Name: "{autoprograms}\{#AppName}\Language Toolchain Manager"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\tools\manage-language-toolchains.ps1"""; WorkingDir: "{app}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: IsAdminInstallMode and NeedsAddPath('{app}'); Tasks: addtopath; Flags: preservestringtype
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: (not IsAdminInstallMode) and NeedsAddPath('{app}'); Tasks: addtopath; Flags: preservestringtype

[Run]
Filename: "{app}\{#AppExeName}"; WorkingDir: "{app}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallDelete]
Type: files; Name: "{app}\build-commit.txt"
Type: files; Name: "{app}\install-scope.txt"

[Code]
procedure InstallUserIntegrations();
var
  ResultCode: Integer;
  Failure: String;
begin
  WizardForm.StatusLabel.Caption := 'Installing Rustic AI agent skills...';
  WizardForm.Refresh;
  Failure := '';
  ResultCode := -1;
  try
    if not ExecAsOriginalUser(ExpandConstant('{app}\rustic-setup-helper.exe'),
      '--install-user-integrations', ExpandConstant('{app}'), SW_HIDE,
      ewWaitUntilTerminated, ResultCode) then
      Failure := Format('Could not start the integration installer: Windows error %d (%s).', [ResultCode, SysErrorMessage(ResultCode)])
    else if ResultCode <> 0 then
      Failure := Format('Agent integration installation exited with code %d (0x%.8x).', [ResultCode, ResultCode]);
  except
    Failure := 'Could not install agent integrations: ' + GetExceptionMessage;
  end;
  if Failure <> '' then
  begin
    Log(Failure);
    SuppressibleMsgBox(Failure + #13#10 + #13#10 +
      'Rustic remains installed. Retry with rustic-agent-backend.exe --install-user-integrations after closing Setup.' + #13#10 +
      'Setup log: ' + ExpandConstant('{log}') + #13#10 +
      'Launch log (if the helper started): %LOCALAPPDATA%\Rustic Game Engine\logs\setup-launch.log',
      mbError, MB_OK, IDOK);
  end;
end;

procedure AddSelectedLanguage(Component, Language: String; var Languages: String);
begin
  if WizardIsComponentSelected(Component) then
  begin
    if Languages <> '' then Languages := Languages + ',';
    Languages := Languages + Language;
  end;
end;

procedure InstallSelectedLanguageToolchains();
var
  Languages: String;
  Failure: String;
  Started: Boolean;
  ResultCode: Integer;
begin
  Languages := '';
  AddSelectedLanguage('languages\python', 'Python', Languages);
  AddSelectedLanguage('languages\csharp', 'CSharp', Languages);
  AddSelectedLanguage('languages\ccpp', 'CCpp', Languages);
  AddSelectedLanguage('languages\java', 'Java', Languages);
  AddSelectedLanguage('languages\php', 'Php', Languages);
  if Languages = '' then Exit;

  WizardForm.StatusLabel.Caption := 'Checking and installing selected gameplay language toolchains...';
  WizardForm.Refresh;

  // The GUI-subsystem helper starts PowerShell with CREATE_NO_WINDOW. SW_HIDE
  // alone still initializes a console host and can fail before the script runs.
  Log('Selected optional language toolchains: ' + Languages);
  Failure := '';
  ResultCode := -1;
  try
    Started := ExecAsOriginalUser(
      ExpandConstant('{app}\rustic-setup-helper.exe'),
      '--install-language-toolchains "' + Languages + '"',
      ExpandConstant('{app}'), SW_HIDE, ewWaitUntilTerminated, ResultCode
    );
    if not Started then
      Failure := Format('Could not start the toolchain setup helper: Windows error %d (%s).', [ResultCode, SysErrorMessage(ResultCode)])
    else if ResultCode <> 0 then
      Failure := Format('Language Toolchain Manager exited with code %d (0x%.8x).', [ResultCode, ResultCode]);
  except
    Failure := 'Could not start the toolchain setup helper: ' + GetExceptionMessage;
  end;

  if Failure <> '' then
  begin
    Log(Failure);
    WizardForm.StatusLabel.Caption := 'Rustic is installed; optional toolchain installation needs attention.';
    // Keep the engine usable and report one failure, including host failures
    // that occur before PowerShell can create its own transcript.
    SuppressibleMsgBox(
      'Rustic Game Engine was installed, but optional language toolchain installation did not finish.' + #13#10 + #13#10 +
      Failure + #13#10 + #13#10 +
      'You can retry from Language Toolchain Manager in the Start menu.' + #13#10 +
      'Setup log: ' + ExpandConstant('{log}') + #13#10 +
      'Launch log (if the helper started): %LOCALAPPDATA%\Rustic Game Engine\logs\setup-launch.log' + #13#10 +
      'Toolchain log (if PowerShell started): %LOCALAPPDATA%\Rustic Game Engine\logs\toolchain-install.log',
      mbError, MB_OK, IDOK);
  end;
  if Failure = '' then
    WizardForm.StatusLabel.Caption := 'Selected gameplay language toolchains are installed.';
  WizardForm.Refresh;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Scope: String;
begin
  if CurStep = ssPostInstall then
  begin
    if IsAdminInstallMode then Scope := 'allusers' else Scope := 'currentuser';
    if not SaveStringToFile(ExpandConstant('{app}\build-commit.txt'), '{#BuildCommit}', False) then
      RaiseException('Could not save installed build identity.');
    if not SaveStringToFile(ExpandConstant('{app}\install-scope.txt'), Scope, False) then
      RaiseException('Could not save installation scope.');
    InstallUserIntegrations();
    InstallSelectedLanguageToolchains();
  end;
end;

function NeedsAddPath(Param: string): Boolean;
var
  CurrentPath: string;
begin
  if IsAdminInstallMode then
    RegQueryStringValue(HKEY_LOCAL_MACHINE, 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment', 'Path', CurrentPath)
  else
    RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', CurrentPath);
  Result := Pos(';' + Uppercase(ExpandConstant(Param)) + ';', ';' + Uppercase(CurrentPath) + ';') = 0;
end;
