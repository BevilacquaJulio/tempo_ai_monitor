<#
  Runs Rust unit tests for the Tauri desktop backend, loading the local .toolchain
  and MSVC/Windows SDK environment automatically when not already on PATH.
  Usage (repository root): npm run test:rust
#>
param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$CargoArgs
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$toolchain = Join-Path $root '.toolchain'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path "$toolchain\cargo\bin\cargo.exe")) {
  $env:CARGO_HOME = "$toolchain\cargo"
  $env:RUSTUP_HOME = "$toolchain\rustup"
  $env:Path = "$toolchain\cargo\bin;$env:Path"
}

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

& cargo test --manifest-path src-tauri/Cargo.toml --locked @CargoArgs
exit $LASTEXITCODE
