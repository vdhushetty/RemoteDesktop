# Remote Desktop - One-line installer for Windows
# Usage: irm https://raw.githubusercontent.com/vdhushetty/RemoteDesktop/main/installer/install.ps1 | iex

$ErrorActionPreference = "Stop"
$REPO = "vdhushetty/RemoteDesktop"
$VERSION = "0.1.0"
$INSTALL_DIR = "$env:ProgramFiles\RemoteDesktop"

Write-Host ""
Write-Host "========================================"
Write-Host "  Remote Desktop Installer v$VERSION"
Write-Host "========================================"
Write-Host ""

# Check admin
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltinRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "Re-launching as Administrator..."
    Start-Process powershell.exe -ArgumentList "-ExecutionPolicy Bypass -File `"$PSCommandPath`"" -Verb RunAs
    exit
}

# Create install directory
New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null

# Try downloading pre-built binaries
$RELEASE_URL = "https://github.com/$REPO/releases/download/v$VERSION"
$agentUrl = "$RELEASE_URL/rd-agent-windows-x86_64.exe"
$viewerUrl = "$RELEASE_URL/rd-viewer-windows-x86_64.exe"

$hasRelease = $false
try {
    $response = Invoke-WebRequest -Uri $agentUrl -Method Head -ErrorAction Stop
    $hasRelease = $true
} catch {
    $hasRelease = $false
}

if ($hasRelease) {
    Write-Host "Downloading pre-built binaries..."
    Invoke-WebRequest -Uri $agentUrl -OutFile "$INSTALL_DIR\rd-agent.exe"
    Invoke-WebRequest -Uri $viewerUrl -OutFile "$INSTALL_DIR\rd-viewer.exe"
} else {
    Write-Host "No pre-built binaries found. Building from source..."
    Write-Host ""

    # Install Rust if needed
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "Installing Rust..."
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile "$env:TEMP\rustup-init.exe"
        Start-Process -FilePath "$env:TEMP\rustup-init.exe" -ArgumentList "-y" -Wait
        $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    }

    # Install Visual Studio Build Tools if needed
    if (-not (Get-Command cl -ErrorAction SilentlyContinue)) {
        Write-Host "Note: Visual Studio Build Tools required. Install from:"
        Write-Host "  https://visualstudio.microsoft.com/visual-cpp-build-tools/"
        Write-Host ""
    }

    # Install vcpkg dependencies
    if (-not (Test-Path "C:\vcpkg")) {
        Write-Host "Installing vcpkg..."
        git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
        & C:\vcpkg\bootstrap-vcpkg.bat
    }
    Write-Host "Installing native libraries..."
    & C:\vcpkg\vcpkg install libvpx:x64-windows opus:x64-windows protobuf:x64-windows
    $env:VCPKG_ROOT = "C:\vcpkg"

    # Clone and build
    $tmpDir = "$env:TEMP\RemoteDesktop-build"
    if (Test-Path $tmpDir) { Remove-Item -Recurse -Force $tmpDir }
    Write-Host "Cloning repository..."
    git clone --depth=1 "https://github.com/$REPO.git" $tmpDir
    Set-Location $tmpDir

    Write-Host "Building (this may take several minutes)..."
    cargo build --release --bin rd-agent --bin rd-viewer

    Copy-Item "target\release\rd-agent.exe" "$INSTALL_DIR\"
    Copy-Item "target\release\rd-viewer.exe" "$INSTALL_DIR\"

    # Cleanup
    Set-Location $env:USERPROFILE
    Remove-Item -Recurse -Force $tmpDir
}

# Add to PATH
$currentPath = [Environment]::GetEnvironmentVariable("Path", "Machine")
if ($currentPath -notlike "*$INSTALL_DIR*") {
    [Environment]::SetEnvironmentVariable("Path", "$currentPath;$INSTALL_DIR", "Machine")
    $env:PATH = "$env:PATH;$INSTALL_DIR"
}

# Create Start Menu shortcuts
$startMenu = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Remote Desktop"
New-Item -ItemType Directory -Force -Path $startMenu | Out-Null

$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut("$startMenu\Remote Desktop Viewer.lnk")
$shortcut.TargetPath = "$INSTALL_DIR\rd-viewer.exe"
$shortcut.Save()

$shortcut = $shell.CreateShortcut("$startMenu\Remote Desktop Agent.lnk")
$shortcut.TargetPath = "$INSTALL_DIR\rd-agent.exe"
$shortcut.Save()

# Desktop shortcut for Viewer
$shortcut = $shell.CreateShortcut("$env:PUBLIC\Desktop\Remote Desktop Viewer.lnk")
$shortcut.TargetPath = "$INSTALL_DIR\rd-viewer.exe"
$shortcut.Save()

# Firewall rule
netsh advfirewall firewall add rule name="Remote Desktop Agent" dir=in action=allow program="$INSTALL_DIR\rd-agent.exe" enable=yes | Out-Null

Write-Host ""
Write-Host "========================================"
Write-Host "  Installation complete!"
Write-Host "========================================"
Write-Host ""
Write-Host "  Shortcuts added to Start Menu and Desktop"
Write-Host ""
Write-Host "  To allow remote access to this machine:"
Write-Host "    rd-agent"
Write-Host ""
Write-Host "  To connect to a remote machine:"
Write-Host "    rd-viewer"
Write-Host ""
Write-Host "  The agent will print a Device ID that"
Write-Host "  you enter in the viewer to connect."
Write-Host ""
