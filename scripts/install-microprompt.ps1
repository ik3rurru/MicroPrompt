# MicroPrompt: install this checkout's release build as the only Hex on
# this Windows account. It replaces the official Hex in daily use and shares
# its data: settings, History, and models stay in %APPDATA%\hex and
# %LOCALAPPDATA%\hex. A local build (HEX_LOCAL_BUILD=1) never updates itself.
#
#   $env:HEX_LOCAL_BUILD = "1"; cargo build --release --bin hex
#   powershell -ExecutionPolicy Bypass -File scripts\install-microprompt.ps1
#
# Then uninstall the official Hex from Settings > Apps if you like; its
# uninstaller keeps your data and leaves this startup entry alone.

$ErrorActionPreference = "Stop"
# The everyday Hex uses the default data folders and the real microphone.
foreach ($name in "HEX_APPLICATION_SUPPORT_DIR", "HEX_AUDIO_FIXTURE", "HEX_STREAM_ATT_RIGHT", "HEX_DISABLE_UPDATES") {
    Remove-Item "Env:$name" -ErrorAction SilentlyContinue
}
$root = Split-Path -Parent $PSScriptRoot
$built = Join-Path $root "target\release\hex.exe"
$dir = Join-Path $env:LOCALAPPDATA "Programs\MicroPrompt"
$exe = Join-Path $dir "hex.exe"
if (-not (Test-Path $built)) { throw "Build it first: cargo build --release --bin hex (with HEX_LOCAL_BUILD=1)" }

# One Hex per data folder: quit whichever one runs (the official, or an older copy of this one).
foreach ($running in Get-Process hex -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -notlike "*\target\*" }) {
    Write-Host "Quitting $($running.Path)"
    & $running.Path quit --force | Out-Null
    $running.WaitForExit(15000) | Out-Null
}

New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $built $exe -Force
Write-Host "Installed $exe"

$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut(
    (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\MicroPrompt.lnk"))
$shortcut.TargetPath = $exe
$shortcut.Arguments = "app"
$shortcut.Save()

Start-Process -FilePath $exe -ArgumentList "app", "--hidden"
$ErrorActionPreference = "Continue"
# It answers once its listener and local API are up.
for ($i = 0; $i -lt 60; $i++) {
    & $exe status 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { break }
    Start-Sleep 1
}
# Off, then on: `on` leaves an existing "Hex" startup entry alone, and the
# official Hex's points at its own executable.
& $exe launch-at-login off | Out-Null
& $exe launch-at-login on
& $exe labs on teleprompter
& $exe status
