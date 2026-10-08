# Install, inspect, and uninstall the packaged per-user installer
# (dist/windows/zeron.iss) silently. Registers and removes the real per-user
# uninstall entry and zerun-dev:// handler, so it refuses to run outside CI unless
# -Force is given.
param(
    [string]$Setup,
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
if (-not $env:CI -and -not $Force) {
    throw 'This test installs and uninstalls Zerun for the current user; pass -Force to run it outside CI'
}
if (-not $Setup) {
    $Setup = Get-ChildItem (Join-Path $PSScriptRoot '../target/package') -Filter 'zerun-*-windows-*-setup.exe' |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $Setup) { throw 'No zerun-*-setup.exe under target/package' }
$Setup = (Resolve-Path -LiteralPath $Setup).Path
$match = [regex]::Match((Split-Path $Setup -Leaf), '\Azerun-(\d+\.\d+\.\d+)-windows-[a-z0-9_]+-setup\.exe\z')
if (-not $match.Success) { throw "Unexpected installer name: $Setup" }
$version = $match.Groups[1].Value
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{94F099C7-A9D5-5A32-B951-50AE16E29A01}_is1'
$protocolKey = 'HKCU:\Software\Classes\zerun-dev'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Zerun.lnk'
$root = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$dir = Join-Path $root "zerun installer test $([guid]::NewGuid().ToString('N'))"
$otherKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{AD5DEC34-E254-467B-8F24-8127EBAF4DA6}_is1'
$seededOtherKey = $false
if ($env:CI -and -not (Test-Path -LiteralPath $otherKey)) {
    New-Item -Path $otherKey -Force | Out-Null
    New-ItemProperty -LiteralPath $otherKey -Name DisplayName -Value 'Zeron' | Out-Null
    New-ItemProperty -LiteralPath $otherKey -Name DisplayVersion -Value '9.9.9' | Out-Null
    $seededOtherKey = $true
}
$otherBefore = if (Test-Path -LiteralPath $otherKey) {
    Get-ItemProperty -LiteralPath $otherKey | ConvertTo-Json -Depth 4 -Compress
} else { '' }

function Assert-OtherRegistration {
    $after = if (Test-Path -LiteralPath $otherKey) {
        Get-ItemProperty -LiteralPath $otherKey | ConvertTo-Json -Depth 4 -Compress
    } else { '' }
    if ($after -ne $otherBefore) { throw 'Installing or uninstalling Zerun changed the other application registration' }
}

function Invoke-Checked([string]$File, [string[]]$Arguments, [string]$What) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "$What exited with $($process.ExitCode)" }
}

function Wait-Until([scriptblock]$Condition, [string]$What) {
    $deadline = (Get-Date).AddSeconds(90)
    while (-not (& $Condition)) {
        if ((Get-Date) -gt $deadline) { throw "Timed out waiting for $What" }
        Start-Sleep -Milliseconds 250
    }
}

$log = Join-Path $root 'zerun-setup.log'
try {
    Invoke-Checked $Setup @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/DIR=`"$dir`"", "/LOG=`"$log`"") 'Setup'
    Assert-OtherRegistration
    $exe = Join-Path $dir 'zerun.exe'
    foreach ($file in @('zerun.exe', 'zerun-update.json', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'licenses/fonts', 'unins000.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $dir $file))) { throw "Installed file missing: $file" }
    }
    Write-Output 'PASS: installed files'

    # The update marker makes the in-app updater manage this install.
    $config = Get-Content -Raw -LiteralPath (Join-Path $dir 'zerun-update.json') | ConvertFrom-Json
    if ($config.application_id -ne 'work.puqing.zerun') { throw 'Update marker belongs to another application' }
    if (-not $config.releases_url.StartsWith('https://')) { throw "Unexpected update feed: $($config.releases_url)" }
    Write-Output 'PASS: update-managed install'

    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $exe
    $info.Arguments = '--version'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $null = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(10000)) { $process.Kill(); throw 'Installed version probe timed out' }
        if ($process.ExitCode -ne 0 -or $stdout.Result.Trim() -ne "zerun $version") {
            throw "Installed executable reports '$($stdout.Result.Trim())', expected 'zerun $version'"
        }
    } finally { $process.Dispose() }
    Write-Output "PASS: installed executable is $version"

    $entry = Get-ItemProperty -LiteralPath $uninstallKey
    if ($entry.DisplayVersion -ne $version) { throw "DisplayVersion is '$($entry.DisplayVersion)', expected '$version'" }
    if ($entry.DisplayName -ne 'Zerun') { throw "DisplayName is '$($entry.DisplayName)'" }
    $installed = [IO.Path]::GetFullPath($entry.InstallLocation).TrimEnd('\')
    if ($installed -ne [IO.Path]::GetFullPath($dir).TrimEnd('\')) { throw "InstallLocation is '$installed'" }
    $command = (Get-ItemProperty -LiteralPath "$protocolKey\shell\open\command").'(default)'
    if ($command -ne "`"$exe`" `"%1`"") { throw "zerun-dev:// handler is '$command'" }
    if (-not (Test-Path -LiteralPath $shortcut)) { throw "Start menu shortcut missing: $shortcut" }
    Write-Output 'PASS: uninstall entry, zerun-dev:// handler, Start menu shortcut'

    # Leftovers an in-app update can leave behind must go with the uninstall.
    Set-Content -LiteralPath (Join-Path $dir 'zerun.exe.old') -Value 'previous image'
    New-Item -ItemType Directory -Path (Join-Path $dir '.zerun-update-test') | Out-Null
    Set-Content -LiteralPath (Join-Path $dir '.zerun-update-test/zerun.exe') -Value 'staged'

    # The uninstaller re-launches itself from a temporary copy and returns early;
    # wait for its effects rather than for the process.
    Invoke-Checked (Join-Path $dir 'unins000.exe') @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') 'Uninstall'
    Wait-Until { -not (Test-Path -LiteralPath $uninstallKey) } 'the uninstall entry to disappear'
    Wait-Until { -not (Test-Path -LiteralPath $exe) } 'zerun.exe to be removed'
    foreach ($leftover in @('zerun.exe.old', '.zerun-update-test', 'zerun-update.json', 'licenses')) {
        Wait-Until { -not (Test-Path -LiteralPath (Join-Path $dir $leftover)) } "$leftover to be removed"
    }
    if (Test-Path -LiteralPath $protocolKey) { throw 'zerun-dev:// handler survived uninstall' }
    if (Test-Path -LiteralPath $shortcut) { throw 'Start menu shortcut survived uninstall' }
    Assert-OtherRegistration
    Write-Output 'PASS: uninstall removes the install, update leftovers, and registrations'
    Write-Output 'PASS: other application registration preserved'
} finally {
    if ($seededOtherKey) { Remove-Item -LiteralPath $otherKey -Force }
}
