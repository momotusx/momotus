; Momotus's Windows installer (plans/windows-app.md 2.4): per user, no administrator, into
; %LOCALAPPDATA%\Programs\Momotus, with a Start menu shortcut and an uninstall entry under HKCU.
; Built by apps/windows/build.sh: makensis -DVERSION=<v> -DSTAGE=<folder> -DOUTFILE=<exe> installer.nsi
Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
  !error "build.sh passes VERSION"
!endif
!ifndef STAGE
  !error "build.sh passes STAGE"
!endif
!ifndef OUTFILE
  !error "build.sh passes OUTFILE"
!endif
!insertmacro GetSize
!insertmacro un.GetSize

!define APP "Momotus"
; The window class of a running Momotus (apps/windows/src/main.rs CLASS).
!define CLASS "Momotus"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP}"

Name "${APP}"
OutFile "${OUTFILE}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
SetCompressor /SOLID lzma
BrandingText "${APP} ${VERSION}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "${APP} Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

!define MUI_ICON "Momotus.ico"
!define MUI_UNICON "Momotus.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\Momotus.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open Momotus"
!define MUI_WELCOMEPAGE_TEXT "This installs Momotus ${VERSION} for you only, in your own folder: no administrator is needed.$\r$\n$\r$\nYour library and settings stay where they are when you install a newer version.$\r$\n$\r$\nClick Next to continue."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!define MUI_FINISHPAGE_TITLE "Momotus is removed"
!define MUI_FINISHPAGE_TEXT "$(KeptText)"
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"
LangString KeptText ${LANG_ENGLISH} "Your library and settings stay in $LOCALAPPDATA\${APP} ($R9 MB), so installing Momotus again keeps them. Delete that folder to free the space."

; A running copy holds its files: ask to close it, then check again. Silent (the app's own update, run as its
; window closes): wait up to 10 s for that window to go.
!macro CloseRunning
  StrCpy $1 0
  close_check:
    FindWindow $0 "${CLASS}" ""
    StrCmp $0 0 close_done
    IfSilent 0 close_ask
    IntOp $1 $1 + 1
    IntCmp $1 40 close_give_up
    Sleep 250
    Goto close_check
  close_ask:
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Close Momotus to continue." IDRETRY close_check
  close_give_up:
    Abort
  close_done:
!macroend

Function .onInit
  !insertmacro CloseRunning
FunctionEnd

Function un.onInit
  !insertmacro CloseRunning
FunctionEnd

Section "Momotus"
  SetOutPath "$INSTDIR"
  ; An update replaces the window's files whole, so none of an older version's stays.
  RMDir /r "$INSTDIR\web"
  File /r "${STAGE}\*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\Momotus.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\Momotus.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0
SectionEnd

Section "Uninstall"
  ; Only what the installer put there: the data folder stays (the last page says where).
  Delete "$INSTDIR\Momotus.exe"
  Delete "$INSTDIR\mq-bridge.exe"
  Delete "$INSTDIR\LICENSE.txt"
  RMDir /r "$INSTDIR\web"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  ${un.GetSize} "$LOCALAPPDATA\${APP}" "/S=0M" $R9 $1 $2
SectionEnd
