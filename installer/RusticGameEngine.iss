#ifndef AddToPathByDefault
  #define AddToPathByDefault "0"
#endif
#ifndef AppVersion
  #define AppVersion "0.1.0"
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
OutputBaseFilename=RusticGameEngine-Setup-{#AppVersion}-x64
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

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
SelectComponentsLabel2=Select the gameplay languages to install. Lua 5.4, JavaScript/QuickJS, and HTML/CSS are bundled. Optional toolchains require an internet connection and can be added later by rerunning Setup or using Language Toolchain Manager from the Start menu.

[Types]
Name: "recommended"; Description: "Recommended installation"
Name: "custom"; Description: "Custom installation"; Flags: iscustom

[Components]
Name: "engine"; Description: "Rustic Game Engine"; Types: recommended custom; Flags: fixed
Name: "languages"; Description: "Gameplay languages (rerun Setup or use Language Toolchain Manager later to change these)"; Types: recommended custom; Flags: fixed
Name: "languages\bundled"; Description: "Lua 5.4, JavaScript/QuickJS, and HTML/CSS (bundled)"; Types: recommended custom; Flags: fixed
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
Source: "..\target\distribution\rustic-project-manager.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-editor.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-runtime.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\distribution\rustic-asset-worker.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\tools\manage-language-toolchains.ps1"; DestDir: "{app}\tools"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autoprograms}\{#AppName}\Language Toolchain Manager"; Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\tools\manage-language-toolchains.ps1"""; WorkingDir: "{app}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: IsAdminInstallMode and NeedsAddPath('{app}'); Tasks: addtopath; Flags: preservestringtype
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: (not IsAdminInstallMode) and NeedsAddPath('{app}'); Tasks: addtopath; Flags: preservestringtype

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
function IsLanguageToolchainInstalled(Language: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := ExecAsOriginalUser(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -ExecutionPolicy Bypass -File "' +
      ExpandConstant('{app}\tools\manage-language-toolchains.ps1') +
      '" -Languages "' + Language + '" -CheckOnly',
    ExpandConstant('{app}'), SW_HIDE, ewWaitUntilTerminated, ResultCode
  ) and (ResultCode = 0);
end;

function SelectedLanguageCount(): Integer;
begin
  Result := 0;
  if WizardIsComponentSelected('languages\python') and not IsLanguageToolchainInstalled('Python') then Result := Result + 1;
  if WizardIsComponentSelected('languages\csharp') and not IsLanguageToolchainInstalled('CSharp') then Result := Result + 1;
  if WizardIsComponentSelected('languages\ccpp') and not IsLanguageToolchainInstalled('CCpp') then Result := Result + 1;
  if WizardIsComponentSelected('languages\java') and not IsLanguageToolchainInstalled('Java') then Result := Result + 1;
  if WizardIsComponentSelected('languages\php') and not IsLanguageToolchainInstalled('Php') then Result := Result + 1;
end;

function AnyOptionalLanguageSelected(): Boolean;
begin
  Result :=
    WizardIsComponentSelected('languages\python') or
    WizardIsComponentSelected('languages\csharp') or
    WizardIsComponentSelected('languages\ccpp') or
    WizardIsComponentSelected('languages\java') or
    WizardIsComponentSelected('languages\php');
end;

procedure InstallLanguageToolchain(Language, DisplayName: String;
  var CompletedCount: Integer; TotalCount: Integer; var Failures: String);
var
  ResultCode: Integer;
begin
  WizardForm.StatusLabel.Caption := Format(
    'Installing %s (%d of %d)...', [DisplayName, CompletedCount + 1, TotalCount]);
  WizardForm.ProgressGauge.Position := CompletedCount;
  WizardForm.Refresh;

  if not ExecAsOriginalUser(
    ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -ExecutionPolicy Bypass -File "' +
      ExpandConstant('{app}\tools\manage-language-toolchains.ps1') +
      '" -Languages "' + Language + '" -LogPath "' +
      ExpandConstant('{localappdata}\Rustic Game Engine\logs\toolchain-install.log') + '"',
    ExpandConstant('{app}'), SW_HIDE, ewWaitUntilTerminated, ResultCode
  ) or (ResultCode <> 0) then
  begin
    if Failures <> '' then Failures := Failures + ', ';
    Failures := Failures + DisplayName;
  end;

  CompletedCount := CompletedCount + 1;
  WizardForm.ProgressGauge.Position := CompletedCount;
  WizardForm.Refresh;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  CompletedCount: Integer;
  TotalCount: Integer;
  Failures: String;
begin
  if CurStep = ssPostInstall then
  begin
    TotalCount := SelectedLanguageCount();
    if TotalCount > 0 then
    begin
      CompletedCount := 0;
      Failures := '';
      WizardForm.ProgressGauge.Min := 0;
      WizardForm.ProgressGauge.Max := TotalCount;
      WizardForm.ProgressGauge.Position := 0;

      if WizardIsComponentSelected('languages\python') and not IsLanguageToolchainInstalled('Python') then
        InstallLanguageToolchain('Python', 'Python 3.14', CompletedCount, TotalCount, Failures);
      if WizardIsComponentSelected('languages\csharp') and not IsLanguageToolchainInstalled('CSharp') then
        InstallLanguageToolchain('CSharp', '.NET SDK 10 for C#', CompletedCount, TotalCount, Failures);
      if WizardIsComponentSelected('languages\ccpp') and not IsLanguageToolchainInstalled('CCpp') then
        InstallLanguageToolchain('CCpp', 'LLVM for C and C++', CompletedCount, TotalCount, Failures);
      if WizardIsComponentSelected('languages\java') and not IsLanguageToolchainInstalled('Java') then
        InstallLanguageToolchain('Java', 'Microsoft OpenJDK 21', CompletedCount, TotalCount, Failures);
      if WizardIsComponentSelected('languages\php') and not IsLanguageToolchainInstalled('Php') then
        InstallLanguageToolchain('Php', 'PHP 8.4 CLI', CompletedCount, TotalCount, Failures);

      WizardForm.StatusLabel.Caption := 'Finished installing gameplay language toolchains.';
      WizardForm.Refresh;

      if Failures <> '' then
        MsgBox(Format(
          'Rustic Game Engine was installed, but these optional language toolchains could not be installed: %s.' + #13#10 + #13#10 +
          'You can retry from Language Toolchain Manager in the Start menu.' + #13#10 +
          'Details: %s', [Failures, ExpandConstant('{localappdata}\Rustic Game Engine\logs\toolchain-install.log')]),
          mbError, MB_OK);
    end;
    if (TotalCount = 0) and AnyOptionalLanguageSelected() then
    begin
      WizardForm.StatusLabel.Caption := 'Selected gameplay language toolchains are already installed.';
      WizardForm.Refresh;
    end;
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
