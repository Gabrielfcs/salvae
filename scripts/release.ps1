# Cut a Salvaê release: bump version, build, make the installer, checksum, and
# publish to GitHub Releases. Usage:  .\scripts\release.ps1 1.2.0
param(
    [Parameter(Mandatory = $true)][string]$Version
)
$ErrorActionPreference = "Stop"
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# 1. Bump the workspace version.
$cargo = Join-Path $root "Cargo.toml"
(Get-Content $cargo) -replace '^version = ".*"', "version = `"$Version`"" | Set-Content $cargo

# 2. Build the release exe.
cargo build --release -p salvae-ui

# 3. Build the installer.
$iscc = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
& $iscc "/DMyAppVersion=$Version" (Join-Path $root "packaging\installer.iss")
$setup = Join-Path $root "packaging\Salvae-Setup.exe"

# 4. Checksum (blake3) next to the installer.
Push-Location (Split-Path $setup)
b3sum (Split-Path $setup -Leaf) | Set-Content "Salvae-Setup.exe.b3" -Encoding ascii
Pop-Location

# 5. Commit the version bump and publish the release.
git add Cargo.toml Cargo.lock
git commit -m "chore: release $Version"
git tag -a "v$Version" -m "Salvaê $Version"
gh release create "v$Version" $setup (Join-Path (Split-Path $setup) "Salvae-Setup.exe.b3") `
    --title "Salvaê $Version" --notes "Salvaê $Version"

Write-Host "Released v$Version. Push with: git push origin master --tags"
