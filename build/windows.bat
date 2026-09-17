@echo off
:: Build and package the native Windows x86_64 release bundle.
::
:: Unlike windows.sh (which cross-compiles from Linux via `cross` + Docker),
:: this runs directly on a Windows machine with a Rust toolchain installed
:: (https://rustup.rs) -- no Docker needed. Requires PowerShell (bundled
:: with Windows 10/Server 2016+) to create the zip and extract llama-server.
::
:: llama-server.exe itself is still the official prebuilt Vulkan build
:: downloaded from llama.cpp's GitHub releases, same as windows.sh -- see
:: LLAMA_RELEASE_TAG in build/common.sh for which release.
setlocal

set "SCRIPT_DIR=%~dp0"
set "ROOT_DIR=%SCRIPT_DIR%.."
set "DIST_DIR=%ROOT_DIR%\dist"
set "APP_NAME=quickai"
set "LLAMA_RELEASE_TAG=b11007"
:: Both bundled models -- Bonsai 8B (recommended default) and Bonsai 1.7B
:: (lighter fallback for slower machines, auto-picked by the GUI based on
:: detected RAM -- see hardware.rs).
set "MODEL1_FILE=Bonsai-8B-Q1_0.gguf"
set "MODEL1_URL=https://huggingface.co/prism-ml/Bonsai-8B-gguf/resolve/main/%MODEL1_FILE%"
set "MODEL2_FILE=Bonsai-1.7B-Q1_0.gguf"
set "MODEL2_URL=https://huggingface.co/prism-ml/Bonsai-1.7B-gguf/resolve/main/%MODEL2_FILE%"

for /f "tokens=2 delims==" %%v in ('findstr /b /r "^version" "%ROOT_DIR%\Cargo.toml"') do set "VERSION=%%v"
set "VERSION=%VERSION: =%"
set "VERSION=%VERSION:"=%"

if not exist "%DIST_DIR%" mkdir "%DIST_DIR%"

:: models/ is gitignored (large binaries, not source) -- fetch them on a
:: fresh checkout if not already there. Downloads to a .part file first so
:: an interrupted download can't leave a truncated file that looks complete.
if not exist "%ROOT_DIR%\models" mkdir "%ROOT_DIR%\models"
call :fetch_model "%MODEL1_FILE%" "%MODEL1_URL%"
if errorlevel 1 exit /b 1
call :fetch_model "%MODEL2_FILE%" "%MODEL2_URL%"
if errorlevel 1 exit /b 1

echo ==^> Building %APP_NAME% %VERSION% (native Windows x86_64)
cargo build --release -p %APP_NAME%
if errorlevel 1 exit /b 1

set "CACHE_DIR=%SCRIPT_DIR%.cache\llama-win-vulkan-%LLAMA_RELEASE_TAG%"
if not exist "%CACHE_DIR%\llama-server.exe" (
    echo ==^> Downloading official llama.cpp Windows Vulkan build %LLAMA_RELEASE_TAG%
    set "ZIP_NAME=llama-%LLAMA_RELEASE_TAG%-bin-win-vulkan-x64.zip"
    set "ZIP_PATH=%SCRIPT_DIR%.cache\%ZIP_NAME%"
    if not exist "%SCRIPT_DIR%.cache" mkdir "%SCRIPT_DIR%.cache"
    if not exist "%CACHE_DIR%" mkdir "%CACHE_DIR%"
    powershell -NoProfile -Command "Invoke-WebRequest -Uri 'https://github.com/ggml-org/llama.cpp/releases/download/%LLAMA_RELEASE_TAG%/%ZIP_NAME%' -OutFile '%ZIP_PATH%'"
    if errorlevel 1 exit /b 1
    powershell -NoProfile -Command "Expand-Archive -Path '%ZIP_PATH%' -DestinationPath '%CACHE_DIR%' -Force"
    if errorlevel 1 exit /b 1
)

set "PKG_NAME=%APP_NAME%-%VERSION%-windows-x86_64"
set "STAGE=%TEMP%\%PKG_NAME%"
if exist "%STAGE%" rmdir /s /q "%STAGE%"
mkdir "%STAGE%\%PKG_NAME%"
mkdir "%STAGE%\%PKG_NAME%\models"

copy /y "%ROOT_DIR%\target\release\%APP_NAME%.exe" "%STAGE%\%PKG_NAME%\" >nul
copy /y "%CACHE_DIR%\*.exe" "%STAGE%\%PKG_NAME%\" >nul
copy /y "%CACHE_DIR%\*.dll" "%STAGE%\%PKG_NAME%\" >nul
copy /y "%ROOT_DIR%\models\%MODEL1_FILE%" "%STAGE%\%PKG_NAME%\models\" >nul
copy /y "%ROOT_DIR%\models\%MODEL2_FILE%" "%STAGE%\%PKG_NAME%\models\" >nul
copy /y "%ROOT_DIR%\build\PACKAGE_README.md" "%STAGE%\%PKG_NAME%\README.md" >nul
copy /y "%ROOT_DIR%\LICENSE" "%STAGE%\%PKG_NAME%\LICENSE" >nul
copy /y "%ROOT_DIR%\THIRD_PARTY_LICENSES.md" "%STAGE%\%PKG_NAME%\THIRD_PARTY_LICENSES.md" >nul

set "ZIP_PATH=%DIST_DIR%\%PKG_NAME%.zip"
if exist "%ZIP_PATH%" del /f /q "%ZIP_PATH%"
powershell -NoProfile -Command "Compress-Archive -Path '%STAGE%\%PKG_NAME%' -DestinationPath '%ZIP_PATH%'"
if errorlevel 1 exit /b 1

rmdir /s /q "%STAGE%"

echo ==^> Wrote %ZIP_PATH%
endlocal
exit /b 0

:fetch_model
:: %1 = filename, %2 = url (both already quoted by the caller)
setlocal
set "FILE=%~1"
set "URL=%~2"
set "PATH_=%ROOT_DIR%\models\%FILE%"
if exist "%PATH_%" (
    endlocal
    exit /b 0
)
echo ==^> Downloading %FILE% (one-time)...
powershell -NoProfile -Command "Invoke-WebRequest -Uri '%URL%' -OutFile '%PATH_%.part'"
if errorlevel 1 (
    endlocal
    exit /b 1
)
move /y "%PATH_%.part" "%PATH_%" >nul
endlocal
exit /b 0
