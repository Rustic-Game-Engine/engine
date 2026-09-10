# Workspace completion requirement

After completing code changes in this repository, build the Windows installer before reporting the task as finished:

```powershell
powershell -ExecutionPolicy Bypass -File .\tools\build-windows-installer.ps1
```

Confirm that a fresh `dist\RusticGameEngine-Setup-*.exe` was produced. If the installer build fails, report the failure and do not describe the task as fully complete.
