@echo off
setlocal
cd /d J:\Caldrayne
set "CARGO_EXE="
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
if exist "I:\Tools\CMake\cmake-4.3.2-windows-x86_64\bin\cmake.exe" (
    set "PATH=I:\Tools\CMake\cmake-4.3.2-windows-x86_64\bin;%PATH%"
)
if exist "I:\Tools\Ninja\bin\ninja.exe" (
    set "PATH=I:\Tools\Ninja\bin;%PATH%"
)
set "CARGO_TARGET_DIR=J:\Caldrayne\.qa\build\public\target"
"%CARGO_EXE%" rustc -p veldr-voxygen --bin caldrayne --no-default-features --features "default-publish,hot-reloading,shaderc-from-source,egui-ui" -- -C link-arg=/DEBUG:NONE
