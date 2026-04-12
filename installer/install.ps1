# Remote Desktop - Windows Installer
# Usage: irm https://raw.githubusercontent.com/vdhushetty/RemoteDesktop/main/installer/install.ps1 | iex

$ErrorActionPreference = "Stop"
$VERSION = "0.1.0"
$INSTALL_DIR = "$env:LOCALAPPDATA\RemoteDesktop"

Write-Host ""
Write-Host "  Remote Desktop Installer v$VERSION"
Write-Host "  =================================="
Write-Host ""

New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null

# Try downloading pre-built binary first
$url = "https://github.com/vdhushetty/RemoteDesktop/releases/download/v$VERSION/RemoteDesktop-windows-x86_64.exe"
$dest = "$INSTALL_DIR\RemoteDesktop.exe"

try {
    Write-Host "  Downloading..."
    Invoke-WebRequest -Uri $url -OutFile $dest -ErrorAction Stop
    Write-Host "  Downloaded successfully."
} catch {
    Write-Host "  Pre-built binary not available. Building from source..."
    Write-Host "  (This requires Rust, LLVM, and vcpkg - may take 10+ minutes)"
    Write-Host ""

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "  Installing Rust..."
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile "$env:TEMP\rustup-init.exe"
        Start-Process -FilePath "$env:TEMP\rustup-init.exe" -ArgumentList "-y" -Wait
        $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    }

    $tmpDir = "$env:TEMP\RemoteDesktop-build"
    if (Test-Path $tmpDir) { Remove-Item -Recurse -Force $tmpDir }
    git clone --depth=1 "https://github.com/vdhushetty/RemoteDesktop.git" $tmpDir
    Set-Location $tmpDir
    cargo build --release --bin rd-desktop
    Copy-Item "target\release\rd-desktop.exe" $dest
    Set-Location $env:USERPROFILE
    Remove-Item -Recurse -Force $tmpDir
}

# Add to PATH
$currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($currentPath -notlike "*$INSTALL_DIR*") {
    [Environment]::SetEnvironmentVariable("Path", "$currentPath;$INSTALL_DIR", "User")
    $env:PATH = "$env:PATH;$INSTALL_DIR"
}

# Create Start Menu shortcut
$startMenu = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs"
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut("$startMenu\Remote Desktop.lnk")
$shortcut.TargetPath = $dest
$shortcut.Save()

# Desktop shortcut
$shortcut = $shell.CreateShortcut("$env:USERPROFILE\Desktop\Remote Desktop.lnk")
$shortcut.TargetPath = $dest
$shortcut.Save()

Write-Host ""
Write-Host "  Installed to: $dest"
Write-Host "  Shortcuts: Start Menu + Desktop"
Write-Host ""
Write-Host "  Run 'Remote Desktop' from Start Menu or Desktop!"
Write-Host ""
