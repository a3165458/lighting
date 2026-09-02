; Lighting NSIS hooks — GlideX-style: install virtual display driver during Setup (one UAC).
!macro customInstall
  DetailPrint "Installing virtual display driver (MttVDD)..."
  ; $INSTDIR\resources\vdd is electron-builder extraResources layout
  nsExec::ExecToLog 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\resources\vdd\provision.ps1" -BundleDir "$INSTDIR\resources\vdd" -ResultFile "$TEMP\lighting-vdd-setup.txt" -Mode Full'
  Pop $0
  DetailPrint "VDD provision exit: $0"
!macroend

!macro customUnInstall
  ; Leave the virtual display driver installed — other apps may use it.
!macroend
