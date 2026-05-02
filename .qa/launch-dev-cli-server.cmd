@echo off
setlocal
set "CALDRAYNE_CONFIG=J:\Caldrayne\.qa\dev"
cd /d J:\Caldrayne
set "CALDRAYNE_EXE=J:\Caldrayne\.qa\build\dev\target\debug\caldrayne.exe"
if not exist "%CALDRAYNE_EXE%" (
    echo Dev client build not found.
    echo Run J:\Caldrayne\.qa\build-dev.cmd first.
    exit /b 1
)
"%CALDRAYNE_EXE%" --product-mode dev --server 203.0.113.10:14004
