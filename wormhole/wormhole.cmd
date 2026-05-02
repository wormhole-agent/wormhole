@echo off
REM wormhole.cmd
REM Convenience launcher used by the watchdog and by hand.
REM Renamed from larry.cmd as part of Lock 3 (2026-05-02).

setlocal
set "WORMHOLE_HOME=%USERPROFILE%\wormhole"
if not exist "%WORMHOLE_HOME%" mkdir "%WORMHOLE_HOME%"

REM Pick the binary. Prefer a release build under .\target\release\, then PATH.
if exist "%~dp0target\release\wormhole.exe" (
  "%~dp0target\release\wormhole.exe" %*
) else (
  wormhole %*
)

endlocal
