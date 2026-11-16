@echo off
rem Device Hub: build and run (DEVHUB_DEMO=1 for demo devices)
cd /d "%~dp0"
echo Building... (first build takes several minutes)
cargo build -p device-hub > build.log 2>&1
if errorlevel 1 (
  echo BUILD FAILED - see build.log
  type build.log | findstr /C:"error"
  pause
  exit /b 1
)
echo BUILD OK > build-ok.txt
start "" target\debug\device-hub.exe
