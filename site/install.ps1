# RalphTerm one-line installer (Windows PowerShell).
#
# Strategy:
#   1. Try the cargo-dist-generated installer from the latest GitHub
#      Release. If a prebuilt binary exists for the caller's platform,
#      we're done.
#   2. If the installer reports no download for the platform, fall back
#      to `cargo install ralphterm`. Requires a Rust toolchain;
#      otherwise print the install instructions and exit.
$ErrorActionPreference = 'Stop'

$installerUrl = 'https://github.com/RayforceDB/ralphterm/releases/latest/download/ralphterm-installer.ps1'
$script = $null
try {
    $script = irm $installerUrl
} catch {
    Write-Error "Failed to download $installerUrl"
}

if ($script) {
    try {
        Invoke-Expression $script
        return
    } catch {
        $msg = $_.Exception.Message
        if ($msg -notmatch "platform|precompiled") {
            throw
        }
        Write-Host "No prebuilt binary for this platform yet. Falling back to: cargo install ralphterm"
    }
}

if (Get-Command cargo -ErrorAction SilentlyContinue) {
    cargo install ralphterm
} else {
    Write-Error @"
Cargo not found. Install Rust from https://rustup.rs and rerun, or see
https://ralphterm.rayforcedb.com/docs/ for manual install options.
"@
}
