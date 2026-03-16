@echo off
setlocal enabledelayedexpansion

REM Resolve script directory and navigate to vmmanager
set "SCRIPT_DIR=%~dp0"
cd /d "%SCRIPT_DIR%\..\apps\vmmanager"
if errorlevel 1 (
    echo ERROR: Could not cd to vmmanager directory
    exit /b 1
)

REM Always clean libcorevm artifacts to avoid stale builds
cargo.exe clean -p libcorevm 2>nul

REM Handle --clean flag
set "RUN_AFTER="
set "RUN_ARGS="
:parse_args
if "%~1"=="" goto done_args
if "%~1"=="--clean" (
    cargo.exe clean
    echo Cleaned build artifacts.
    shift
    goto parse_args
)
if "%~1"=="--run" (
    set "RUN_AFTER=1"
    shift
    :collect_run_args
    if "%~1"=="" goto done_args
    set "RUN_ARGS=!RUN_ARGS! %~1"
    shift
    goto collect_run_args
)
shift
goto parse_args
:done_args

cargo.exe +stable build --release --target x86_64-pc-windows-msvc --features libcorevm/windows
if errorlevel 1 exit /b 1
echo Built: target\x86_64-pc-windows-msvc\release\corevm-vmmanager.exe

if defined RUN_AFTER (
    echo Running corevm-vmmanager...
    cargo.exe +stable run --release --target x86_64-pc-windows-msvc --features libcorevm/windows -- %RUN_ARGS%
)
