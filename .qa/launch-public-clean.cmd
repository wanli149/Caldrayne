@echo off
setlocal
for %%I in ("%~dp0..") do set "CALDRAYNE_ROOT=%%~fI"
set "CALDRAYNE_CONFIG=%CALDRAYNE_ROOT%\.qa\public-clean"
cd /d "%CALDRAYNE_ROOT%"
set "CALDRAYNE_EXE=%CALDRAYNE_ROOT%\.qa\build\public\target\debug\caldrayne.exe"
if not exist "%CALDRAYNE_EXE%" (
    echo Public client build not found.
    echo Run "%CALDRAYNE_ROOT%\.qa\build-public.cmd" first.
    exit /b 1
)
start "" "%CALDRAYNE_EXE%" --product-mode public
