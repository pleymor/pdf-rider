# Build MSIX package for Microsoft Store submission
# Requires Windows SDK (makeappx.exe)

param(
    [switch]$SkipBuild,
    [string]$Version = "",
    [string]$Architecture = "x64"
)

$ErrorActionPreference = "Stop"

# Configuration
$AppName = "pdf-rider"
$ExeName = "pdf-reader-portable"
$OutputDir = ".\msix-output"
$PackageDir = "$OutputDir\package"

# Get version from tauri.conf.json if not provided
if (-not $Version) {
    $conf = Get-Content ".\src-tauri\tauri.conf.json" | ConvertFrom-Json
    $Version = $conf.version
}
# MSIX requires 4-part version
if (($Version -split '\.').Count -lt 4) {
    $Version = "$Version.0"
}

Write-Host "Building MSIX v$Version" -ForegroundColor Cyan

# Find makeappx.exe from Windows SDK
$makeappx = $null
$sdkBase = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
if (Test-Path $sdkBase) {
    $versions = Get-ChildItem $sdkBase -Directory | Sort-Object Name -Descending
    foreach ($ver in $versions) {
        $candidate = Join-Path $ver.FullName "x64\makeappx.exe"
        if (Test-Path $candidate) {
            $makeappx = $candidate
            break
        }
    }
}

if (-not $makeappx) {
    Write-Error "makeappx.exe not found. Please install Windows SDK from https://developer.microsoft.com/windows/downloads/windows-sdk/"
    exit 1
}

Write-Host "Using makeappx: $makeappx" -ForegroundColor Cyan

# Step 1: Build Tauri app
if (-not $SkipBuild) {
    Write-Host "`n=== Building Tauri application ===" -ForegroundColor Green
    npm run tauri build
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Tauri build failed"
        exit 1
    }
}

# Step 2: Create package directory structure
Write-Host "`n=== Creating MSIX package structure ===" -ForegroundColor Green

if (Test-Path $OutputDir) {
    Remove-Item $OutputDir -Recurse -Force
}
New-Item -ItemType Directory -Path $PackageDir -Force | Out-Null
New-Item -ItemType Directory -Path "$PackageDir\icons" -Force | Out-Null

# Step 3: Copy built application files
$buildDir = ".\src-tauri\target\release"
if (-not (Test-Path $buildDir)) {
    Write-Error "Build directory not found: $buildDir"
    exit 1
}

Write-Host "Copying application files..." -ForegroundColor Yellow
Copy-Item "$buildDir\$ExeName.exe" "$PackageDir\" -Force

# Copy any DLLs
Get-ChildItem "$buildDir\*.dll" | ForEach-Object {
    Copy-Item $_.FullName "$PackageDir\" -Force
}

# Step 4: Copy icons
Write-Host "Copying icons..." -ForegroundColor Yellow
$iconFiles = @(
    "StoreLogo.png",
    "Square44x44Logo.png",
    "Square71x71Logo.png",
    "Square150x150Logo.png",
    "Square310x310Logo.png"
)

foreach ($icon in $iconFiles) {
    $src = ".\src-tauri\icons\$icon"
    if (Test-Path $src) {
        Copy-Item $src "$PackageDir\icons\" -Force
    } else {
        Write-Warning "Icon not found: $icon"
    }
}

# Step 5: Copy and update manifest with version
Write-Host "Copying manifest..." -ForegroundColor Yellow
$manifest = Get-Content ".\src-tauri\AppxManifest.xml" -Raw
$manifest = $manifest -replace 'Version="[^"]*"', "Version=`"$Version`""
$manifest | Set-Content "$PackageDir\AppxManifest.xml" -Encoding utf8

# Step 6: Create MSIX package
Write-Host "`n=== Creating MSIX package ===" -ForegroundColor Green
$msixFile = "$OutputDir\PDFRider_${Version}_${Architecture}.msix"

& $makeappx pack /d $PackageDir /p $msixFile /o

if ($LASTEXITCODE -ne 0) {
    Write-Error "makeappx failed"
    exit 1
}

Write-Host "`n=== Build Complete ===" -ForegroundColor Green
Write-Host "MSIX package created: $msixFile" -ForegroundColor Cyan
