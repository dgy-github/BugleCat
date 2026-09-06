@echo off
setlocal
cd /d "%~dp0.."
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-eval.ps1" %*
if errorlevel 1 (
  echo.
  echo Evaluation failed. Press any key to close.
  pause >nul
  exit /b %errorlevel%
)
echo.
echo Evaluation complete.
pause
