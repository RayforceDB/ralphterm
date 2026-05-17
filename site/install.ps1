# RalphTerm one-line installer (Windows PowerShell).
# Downloads and runs the cargo-dist-generated installer from the latest GitHub release.
$ErrorActionPreference = 'Stop'
$installerUrl = 'https://github.com/RayforceDB/ralphterm/releases/latest/download/ralphterm-installer.ps1'
iex (irm $installerUrl)
