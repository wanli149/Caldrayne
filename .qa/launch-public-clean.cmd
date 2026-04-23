@echo off
setlocal
set "VOXYGEN_CONFIG=J:\Caldrayne\.qa\public-clean"
cd /d J:\Caldrayne
set "VOXYGEN_EXE=J:\Caldrayne\.qa\build\public\target\debug\veloren-voxygen.exe"
if not exist "%VOXYGEN_EXE%" (
    echo Public client build not found.
    echo Run J:\Caldrayne\.qa\build-public.cmd first.
    exit /b 1
)
start "" "%VOXYGEN_EXE%" --product-mode public
