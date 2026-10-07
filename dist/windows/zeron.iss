; Zerun for Windows — per-user installer (Inno Setup 6).
;
; Built by scripts/package-windows.ps1, which passes the version, the package
; architecture, and the staged portable directory:
;   ISCC.exe /DAppVersion=0.2.97 /DArch=x86_64 /DPackageDir=<stage> /DOutputDir=<out> zeron.iss
;
; Installs into %LOCALAPPDATA%\Programs\Zerun without elevation, like VS
; Code's user setup: the directory stays writable by its user, so the in-app
; updater (crates/update/src/windows.rs) can replace zeron.exe in place. The
; staged directory already carries zeron-update.json, which marks the install
; as update-managed. Re-running a newer installer upgrades in place; user data
; lives in %LOCALAPPDATA%\Zeron and is never touched here.

#ifndef AppVersion
  #error AppVersion must be defined (/DAppVersion=x.y.z)
#endif
#ifndef Arch
  #error Arch must be defined (/DArch=x86_64 or /DArch=aarch64)
#endif
#ifndef PackageDir
  #error PackageDir must be defined (/DPackageDir=<staged package directory>)
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

#if Arch == "aarch64"
  #define ArchAllowed "arm64"
#else
  #define ArchAllowed "x64compatible"
#endif

[Setup]
; Never change AppId: it identifies the installation across upgrades, and
; crates/update/src/windows.rs refreshes DisplayVersion under this key after
; in-app updates.
AppId={{AD5DEC34-E254-467B-8F24-8127EBAF4DA6}
AppName=Zerun
AppVersion={#AppVersion}
AppVerName=Zerun {#AppVersion}
AppPublisher=Zerun
AppPublisherURL=https://github.com/AndPuQing/zeron
AppSupportURL=https://github.com/AndPuQing/zeron/issues
AppUpdatesURL=https://github.com/AndPuQing/zeron/releases
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
DefaultDirName={autopf}\Zerun
DisableProgramGroupPage=yes
DisableDirPage=auto
DisableReadyPage=yes
ArchitecturesAllowed={#ArchAllowed}
ArchitecturesInstallIn64BitMode={#ArchAllowed}
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=zeron-{#AppVersion}-windows-{#Arch}-setup
SetupIconFile=zeron.ico
UninstallDisplayIcon={app}\zeron.exe
UninstallDisplayName=Zerun
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; A running Zerun is closed through the Restart Manager before its files are
; replaced; the updated app starts again from the finish page.
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PackageDir}\zeron.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PackageDir}\zeron-update.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PackageDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PackageDir}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PackageDir}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Zerun"; Filename: "{app}\zeron.exe"
Name: "{autodesktop}\Zerun"; Filename: "{app}\zeron.exe"; Tasks: desktopicon

[Registry]
; zerun-dev:// conversation links — registered here and in macOS Info.plist.
Root: HKCU; Subkey: "Software\Classes\zerun-dev"; ValueType: string; ValueName: ""; ValueData: "URL:Zerun"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\zerun-dev"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\zerun-dev\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\zeron.exe"",0"
Root: HKCU; Subkey: "Software\Classes\zerun-dev\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\zeron.exe"" ""%1"""

[Run]
Filename: "{app}\zeron.exe"; Description: "{cm:LaunchProgram,Zerun}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Leftovers of in-app updates (crates/update/src/windows.rs).
Type: files; Name: "{app}\zeron.exe.old"
Type: files; Name: "{app}\.zeron-update-incoming.exe"
Type: filesandordirs; Name: "{app}\.zeron-update-*"
