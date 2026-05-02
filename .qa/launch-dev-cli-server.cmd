@echo off
setlocal
for %%I in ("%~dp0..") do set "CALDRAYNE_ROOT=%%~fI"
set "CALDRAYNE_CONFIG=%CALDRAYNE_ROOT%\.qa\dev"
cd /d "%CALDRAYNE_ROOT%"
set "CALDRAYNE_EXE=%CALDRAYNE_ROOT%\.qa\build\dev\target\debug\caldrayne.exe"
if not exist "%CALDRAYNE_EXE%" (
    echo Dev client build not found.
    echo Run "%CALDRAYNE_ROOT%\.qa\build-dev.cmd" first.
    exit /b 1
)
"%CALDRAYNE_EXE%" --product-mode dev --server 127.0.0.1:14004
