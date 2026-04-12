!include "MUI2.nsh"

; Application metadata
!define APP_NAME "Remote Desktop"
!define APP_VERSION "0.1.0"
!define APP_PUBLISHER "RemoteDesktop"
!define APP_URL "https://github.com/vdhushetty/RemoteDesktop"
!define INSTALL_DIR "$PROGRAMFILES\${APP_NAME}"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "RemoteDesktop-Setup-${APP_VERSION}.exe"
InstallDir "${INSTALL_DIR}"
RequestExecutionLevel admin

; UI settings
!define MUI_ICON "..\..\assets\icons\icon.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "Welcome to ${APP_NAME} Setup"
!define MUI_WELCOMEPAGE_TEXT "This will install ${APP_NAME} on your computer.$\r$\n$\r$\nFeatures:$\r$\n- Remote desktop control (view + mouse/keyboard)$\r$\n- File transfer$\r$\n- Clipboard sync$\r$\n- Audio streaming$\r$\n- Works over LAN and Internet"

; Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Component: Agent (allow remote access to this machine)
Section "Remote Desktop Agent" SecAgent
    SetOutPath "$INSTDIR"
    File "rd-agent.exe"

    ; Create Start Menu shortcuts
    CreateDirectory "$SMPROGRAMS\${APP_NAME}"
    CreateShortcut "$SMPROGRAMS\${APP_NAME}\Remote Desktop Agent.lnk" "$INSTDIR\rd-agent.exe"

    ; Add to startup (run on boot)
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "RemoteDesktopAgent" "$INSTDIR\rd-agent.exe"
SectionEnd

; Component: Viewer (connect to remote machines)
Section "Remote Desktop Viewer" SecViewer
    SetOutPath "$INSTDIR"
    File "rd-viewer.exe"

    CreateDirectory "$SMPROGRAMS\${APP_NAME}"
    CreateShortcut "$SMPROGRAMS\${APP_NAME}\Remote Desktop Viewer.lnk" "$INSTDIR\rd-viewer.exe"
    CreateShortcut "$DESKTOP\Remote Desktop Viewer.lnk" "$INSTDIR\rd-viewer.exe"
SectionEnd

; Shared: uninstaller + registry
Section "-Shared"
    SetOutPath "$INSTDIR"

    ; Write uninstaller
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Add to Add/Remove Programs
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "DisplayVersion" "${APP_VERSION}"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "Publisher" "${APP_PUBLISHER}"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "URLInfoAbout" "${APP_URL}"
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}" "NoModify" 1

    ; Firewall rule
    nsExec::Exec 'netsh advfirewall firewall add rule name="Remote Desktop Agent" dir=in action=allow program="$INSTDIR\rd-agent.exe" enable=yes'

    CreateShortcut "$SMPROGRAMS\${APP_NAME}\Uninstall.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

; Component descriptions
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SecAgent} "Install the agent to allow remote access to this machine. Starts automatically on boot."
    !insertmacro MUI_DESCRIPTION_TEXT ${SecViewer} "Install the viewer to connect to and control other machines."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; Uninstaller
Section "Uninstall"
    ; Kill running processes
    nsExec::Exec 'taskkill /f /im rd-agent.exe'
    nsExec::Exec 'taskkill /f /im rd-viewer.exe'

    ; Remove startup entry
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "RemoteDesktopAgent"

    ; Remove firewall rule
    nsExec::Exec 'netsh advfirewall firewall delete rule name="Remote Desktop Agent"'

    ; Remove files
    Delete "$INSTDIR\rd-agent.exe"
    Delete "$INSTDIR\rd-viewer.exe"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    ; Remove shortcuts
    Delete "$SMPROGRAMS\${APP_NAME}\*.lnk"
    RMDir "$SMPROGRAMS\${APP_NAME}"
    Delete "$DESKTOP\Remote Desktop Viewer.lnk"

    ; Remove registry
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
SectionEnd
