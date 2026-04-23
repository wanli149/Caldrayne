@echo off
setlocal
set "VOXYGEN_CONFIG=J:\Caldrayne\.qa\dev"
cd /d J:\Caldrayne
set "VOXYGEN_EXE=J:\Caldrayne\.qa\build\dev\target\debug\veloren-voxygen.exe"
if not exist "%VOXYGEN_EXE%" (
    echo Dev client build not found.
    echo Run J:\Caldrayne\.qa\build-dev.cmd first.
    exit /b 1
)
"%VOXYGEN_EXE%" --product-mode dev
