<#
  Builds TempoAI for Windows and puts the two files people need in release/:
    TempoAI-<version>-setup.exe     installer (recommended)
    TempoAI-<version>-portable.exe  runs without installing
  Usage (repository root): npm run release
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Rust and the Windows SDK may live in .toolchain when they are not installed system-wide.
$toolchain = Join-Path $root '.toolchain'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path "$toolchain\cargo\bin\cargo.exe")) {
  $env:CARGO_HOME = "$toolchain\cargo"
  $env:RUSTUP_HOME = "$toolchain\rustup"
  $env:Path = "$toolchain\cargo\bin;$env:Path"
}

# The MSVC compiler only works after its environment script runs; import it into this session.
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  $vcvars = if (Test-Path $vswhere) {
    $install = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($install) { Join-Path $install 'VC\Auxiliary\Build\vcvars64.bat' }
  }
  if (-not $vcvars -or -not (Test-Path $vcvars)) {
    $vcvars = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft Visual Studio", "$env:ProgramFiles\Microsoft Visual Studio" -Filter vcvars64.bat -Recurse -ErrorAction SilentlyContinue |
      Select-Object -First 1 -ExpandProperty FullName
  }
  if (-not $vcvars) { throw 'Visual Studio Build Tools (Desktop development with C++) not found.' }
  cmd /c "`"$vcvars`" >nul && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2] }
  }
}

$sdkInclude = "$toolchain\microsoft.windows.sdk.cpp\c\Include"
if (-not (Test-Path "${env:ProgramFiles(x86)}\Windows Kits\10\Lib") -and (Test-Path $sdkInclude)) {
  $version = (Get-ChildItem $sdkInclude -Directory | Sort-Object Name -Descending | Select-Object -First 1).Name
  $include = Join-Path $sdkInclude $version
  $lib = "$toolchain\microsoft.windows.sdk.cpp.x64\c"
  $env:INCLUDE = "$env:INCLUDE;$include\ucrt;$include\um;$include\shared;$include\winrt"
  $env:LIB = "$env:LIB;$lib\ucrt\x64;$lib\um\x64"
  $env:Path = "$toolchain\microsoft.windows.sdk.cpp\c\bin\$version\x64;$env:Path"
}

$version = (Get-Content package.json -Raw | ConvertFrom-Json).version
Write-Host "Building TempoAI $version..." -ForegroundColor Cyan
npm.cmd run tauri -- build
if ($LASTEXITCODE -ne 0) { throw "Build failed (exit code $LASTEXITCODE)." }

$target = Join-Path $root 'src-tauri\target\release'
$installer = Get-ChildItem "$target\bundle\nsis" -Filter "*_${version}_x64-setup.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $installer) { throw 'Installer not found in src-tauri\target\release\bundle\nsis.' }

$release = Join-Path $root 'release'
# A running copy locks its own .exe; say so plainly instead of failing halfway.
$running = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($release, [StringComparison]::OrdinalIgnoreCase) }
if ($running) { throw "Close TempoAI (opened from release\) and run npm run release again. The new build is ready; only the copy to release\ is pending." }
# Only files this script creates live here, so older versions never sit next to the new one.
if (Test-Path $release) { Get-ChildItem "$release\*" -Include 'TempoAI-*.exe','SHA256SUMS.txt' -File | Remove-Item -Force }
New-Item -ItemType Directory -Force $release | Out-Null
$setupPath = Join-Path $release "TempoAI-$version-setup.exe"
$portablePath = Join-Path $release "TempoAI-$version-portable.exe"
Copy-Item $installer.FullName $setupPath
Copy-Item "$target\tempo-ai.exe" $portablePath

$sha256 = [System.Security.Cryptography.SHA256]::Create()
try {
  $checksumLines = Get-ChildItem $release -Filter 'TempoAI-*.exe' | Sort-Object Name | ForEach-Object {
    $stream = [System.IO.File]::OpenRead($_.FullName)
    try {
      $hash = ([System.BitConverter]::ToString($sha256.ComputeHash($stream)) -replace '-', '').ToLowerInvariant()
    } finally {
      $stream.Dispose()
    }
    "$hash *$($_.Name)"
  }
} finally {
  $sha256.Dispose()
}
$checksumPath = Join-Path $release 'SHA256SUMS.txt'
Set-Content -Path $checksumPath -Value $checksumLines -Encoding ascii

Write-Host "`nReady in release\:" -ForegroundColor Green
Get-ChildItem $release -Filter 'TempoAI-*.exe' | Sort-Object Name | ForEach-Object { '  {0}  ({1:N1} MB)' -f $_.Name, ($_.Length / 1MB) }
Write-Host "  SHA256SUMS.txt" -ForegroundColor Gray
$checksumLines | ForEach-Object { "  $_" }
