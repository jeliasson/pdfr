# Builds the per-user installer from target\release\pdfr.exe.
#   packaging\windows\build-msi.ps1 -Version 0.1.0 -Out dist
param([Parameter(Mandatory)] [string] $Version, [string] $Out = "dist")
$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\.."
New-Item -ItemType Directory -Force $Out | Out-Null
dotnet tool install --global wix --version 5.* 2>$null
$v = $Version -replace '-.*$', ''   # MSI versions are numbers only
Push-Location "$root\packaging\windows"
wix build pdfr.wxs -d Version=$v -d BinDir="$root\target\release" -arch x64 -o "$root\$Out\pdfr-v$Version-windows-x64.msi"
Pop-Location
Copy-Item "$root\target\release\pdfr.exe" "$root\$Out\pdfr-v$Version-windows-x64.exe"
Get-ChildItem $Out
