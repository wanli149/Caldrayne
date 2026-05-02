@echo off
setlocal
set "CALDRAYNE_CONFIG=J:\Caldrayne\.qa\public-clean"
cd /d J:\Caldrayne
set "CALDRAYNE_EXE=J:\Caldrayne\.qa\build\public\target\debug\caldrayne.exe"
if not exist "%CALDRAYNE_EXE%" (
    echo Public client build not found.
    echo Run J:\Caldrayne\.qa\build-public.cmd first.
    exit /b 1
)
start "" "%CALDRAYNE_EXE%" --product-mode public
