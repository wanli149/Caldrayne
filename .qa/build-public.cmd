@echo off
setlocal
for %%I in ("%~dp0..") do set "CALDRAYNE_ROOT=%%~fI"
cd /d "%CALDRAYNE_ROOT%"
set "CARGO_EXE="
if defined CALDRAYNE_CARGO_EXE if exist "%CALDRAYNE_CARGO_EXE%" set "CARGO_EXE=%CALDRAYNE_CARGO_EXE%"
for /f "delims=" %%I in ('where cargo.exe 2^>nul') do if not defined CARGO_EXE set "CARGO_EXE=%%I"
if not defined CARGO_EXE (
    for /d %%D in ("%USERPROFILE%\.rustup\toolchains\*") do (
        if not defined CARGO_EXE if exist "%%~fD\bin\cargo.exe" set "CARGO_EXE=%%~fD\bin\cargo.exe"
    )
)
if not defined CARGO_EXE (
    echo Cargo executable not found.
    exit /b 1
)
for %%I in ("%CARGO_EXE%") do set "RUST_BIN_DIR=%%~dpI"
set "PATH=%RUST_BIN_DIR%;%PATH%"
set "CMAKE_EXE="
if defined CALDRAYNE_CMAKE_EXE if exist "%CALDRAYNE_CMAKE_EXE%" set "CMAKE_EXE=%CALDRAYNE_CMAKE_EXE%"
for /f "delims=" %%I in ('where cmake.exe 2^>nul') do if not defined CMAKE_EXE set "CMAKE_EXE=%%I"
if not defined CMAKE_EXE if exist "%ProgramFiles%\CMake\bin\cmake.exe" set "CMAKE_EXE=%ProgramFiles%\CMake\bin\cmake.exe"
if not defined CMAKE_EXE if defined ProgramFiles(x86) if exist "%ProgramFiles(x86)%\CMake\bin\cmake.exe" set "CMAKE_EXE=%ProgramFiles(x86)%\CMake\bin\cmake.exe"
if defined CMAKE_EXE (
    for %%I in ("%CMAKE_EXE%") do set "PATH=%%~dpI;%PATH%"
)
if not defined CMAKE_EXE (
    echo CMake executable not found.
    echo Set CALDRAYNE_CMAKE_EXE or install cmake.exe in PATH or a standard location.
    exit /b 1
)
set "NINJA_EXE="
if defined CALDRAYNE_NINJA_EXE if exist "%CALDRAYNE_NINJA_EXE%" set "NINJA_EXE=%CALDRAYNE_NINJA_EXE%"
for /f "delims=" %%I in ('where ninja.exe 2^>nul') do if not defined NINJA_EXE set "NINJA_EXE=%%I"
if not defined NINJA_EXE if defined ChocolateyInstall if exist "%ChocolateyInstall%\bin\ninja.exe" set "NINJA_EXE=%ChocolateyInstall%\bin\ninja.exe"
if not defined NINJA_EXE if exist "%USERPROFILE%\scoop\shims\ninja.exe" set "NINJA_EXE=%USERPROFILE%\scoop\shims\ninja.exe"
if defined NINJA_EXE (
    for %%I in ("%NINJA_EXE%") do set "PATH=%%~dpI;%PATH%"
)
set "CARGO_TARGET_DIR=%CALDRAYNE_ROOT%\.qa\build\public\target"
"%CARGO_EXE%" rustc -p veldr-voxygen --bin caldrayne --no-default-features --features "default-publish,hot-reloading,shaderc-from-source,egui-ui" -- -C link-arg=/DEBUG:NONE
