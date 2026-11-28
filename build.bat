@echo off
rem Double-click helper for build-bot. The questions (schematic, server, name) are asked by the
rem program itself: this file never puts typed text on a command line, so nothing typed or pasted
rem here can run as a command. Keep it next to build-bot.exe, or in the repository after
rem "cargo build --release".
setlocal DisableDelayedExpansion
cd /d "%~dp0"
set "BOT=%~dp0build-bot.exe"
if not exist "%BOT%" set "BOT=%~dp0target\release\build-bot.exe"
if not exist "%BOT%" goto missing
"%BOT%" build --ask
goto end

:missing
echo build-bot.exe was not found next to this file.
echo Download the Windows zip from the Releases page, or run: cargo build --release

:end
echo.
pause
endlocal
