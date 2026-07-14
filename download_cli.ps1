##############################################################################
# bharatcode CLI Install Script for Windows PowerShell
#
# This script downloads the latest stable 'bharatcode' CLI binary from GitHub releases
# and installs it to your system.
#
# Supported OS: Windows
# Supported Architectures: x86_64
#
# Usage:
#   Invoke-WebRequest -Uri "https://github.com/arbazkhan971/bharatcode-cli/releases/download/stable/download_cli.ps1" -OutFile "download_cli.ps1"; .\download_cli.ps1
#   Or simply: .\download_cli.ps1
#
# Environment variables:
#   $env:BHARATCODE_BIN_DIR  - Directory to which bharatcode will be installed (default: $env:USERPROFILE\.local\bin)
#   $env:BHARATCODE_VERSION  - Optional: specific version to install (e.g., "v1.0.25"). Can be in the format vX.Y.Z, vX.Y.Z-suffix, or X.Y.Z
#   $env:BHARATCODE_PROVIDER - Optional: provider for bharatcode
#   $env:BHARATCODE_MODEL    - Optional: model for bharatcode
#   $env:BHARATCODE_WINDOWS_VARIANT - Optional: Windows package variant to install ("standard" or "cuda")
#   $env:CANARY         - Optional: if set to "true", downloads from canary release instead of stable
#   $env:CONFIGURE      - Optional: if set to "false", disables running bharatcode configure interactively
#   $env:BHARATCODE_ALLOW_UNVERIFIED - Optional: if "true", install even when the release publishes no
#                         checksums.txt. Only needed to install a pinned version older than the release
#                         that introduced the manifest. It never bypasses a checksum *mismatch*.
#
# Integrity:
#   Every release publishes checksums.txt, a sha256sum-format manifest covering every asset in
#   that release. This script downloads the manifest from the same release as the archive and
#   verifies the archive against it BEFORE extracting or executing anything from it.
#
#   Stable and pinned-version installs fail closed: a missing manifest, or a missing entry for the
#   archive, aborts the install. Canary is exempt from the missing-manifest case only because
#   The explicit legacy-release escape hatch may waive a missing manifest; a mismatch is always fatal.
##############################################################################

# Set error action preference to stop on errors
$ErrorActionPreference = "Stop"

# --- 0) Integrity helpers ---
function Get-Sha256Hex {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

# Return the expected digest for exactly $Name from an sha256sum-format manifest, else $null.
# Matching is on the whole filename: a request for "bharatcode.zip" must not match an entry
# for "bharatcode-x86_64-pc-windows-msvc.zip".
function Get-ChecksumFromManifest {
    param(
        [Parameter(Mandatory = $true)][string]$ManifestPath,
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$Name
    )
    if ([string]::IsNullOrWhiteSpace($Name)) { return $null }
    foreach ($line in (Get-Content -LiteralPath $ManifestPath)) {
        $fields = @(($line -split '\s+') | Where-Object { $_ -ne '' })
        if ($fields.Count -lt 2) { continue }
        $digest = $fields[0]
        $file = $fields[1] -replace '^\*', '' -replace '^\./', ''
        if (($file -ceq $Name) -and ($digest -match '^[0-9a-fA-F]{64}$')) {
            return $digest.ToLowerInvariant()
        }
    }
    return $null
}

# 0 = verified, 1 = MISMATCH, 2 = no entry in manifest
function Test-ArchiveChecksum {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ManifestPath,
        [Parameter(Mandatory = $true)][string]$Name
    )
    $expected = Get-ChecksumFromManifest -ManifestPath $ManifestPath -Name $Name
    if (-not $expected) { return 2 }
    if ((Get-Sha256Hex -Path $Path) -ne $expected) { return 1 }
    return 0
}

# --- 0b) Self-test ($env:BHARATCODE_SELF_TEST = "true") ---
# Exercises the integrity helpers above without touching the network.
# Run: $env:BHARATCODE_SELF_TEST="true"; .\download_cli.ps1
if ($env:BHARATCODE_SELF_TEST -eq "true") {
    $script:passed = 0
    $script:failed = 0
    function Assert-Equal {
        param($Description, $Expected, $Actual)
        if ($Expected -eq $Actual) {
            $script:passed++
            Write-Host "  ok   - $Description" -ForegroundColor Green
        } else {
            $script:failed++
            Write-Host "  FAIL - $Description (expected '$Expected', got '$Actual')" -ForegroundColor Red
        }
    }

    $testDir = Join-Path $env:TEMP "bharatcode_selftest_$(Get-Random)"
    New-Item -ItemType Directory -Path $testDir -Force | Out-Null
    try {
        $payload = Join-Path $testDir "payload"
        # Write "hello world" with no trailing newline, whose SHA-256 is known.
        [System.IO.File]::WriteAllText($payload, "hello world", (New-Object System.Text.UTF8Encoding $false))
        $hw = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        $zeros = "0" * 64

        Assert-Equal "Get-Sha256Hex matches known digest" $hw (Get-Sha256Hex -Path $payload)

        $manifest = Join-Path $testDir "checksums.txt"
        @(
            "$hw  bharatcode-x86_64-pc-windows-msvc.zip",
            "$zeros  bharatcode-x86_64-pc-windows-msvc-cuda.zip",
            "$hw  *bharatcode-starred.zip",
            "$hw  ./bharatcode-dotslash.zip",
            "not-a-digest  bharatcode-bogus.zip"
        ) | Set-Content -LiteralPath $manifest

        Assert-Equal "manifest lookup finds entry" $hw `
            (Get-ChecksumFromManifest -ManifestPath $manifest -Name "bharatcode-x86_64-pc-windows-msvc.zip")
        Assert-Equal "manifest lookup strips binary-mode marker" $hw `
            (Get-ChecksumFromManifest -ManifestPath $manifest -Name "bharatcode-starred.zip")
        Assert-Equal "manifest lookup strips ./ prefix" $hw `
            (Get-ChecksumFromManifest -ManifestPath $manifest -Name "bharatcode-dotslash.zip")
        Assert-Equal "manifest lookup ignores malformed digest" $null `
            (Get-ChecksumFromManifest -ManifestPath $manifest -Name "bharatcode-bogus.zip")
        Assert-Equal "manifest lookup misses absent asset" $null `
            (Get-ChecksumFromManifest -ManifestPath $manifest -Name "bharatcode-not-published.zip")
        # A suffix/substring match here would let an attacker's asset satisfy the check.
        Assert-Equal "manifest lookup does not substring-match" $null `
            (Get-ChecksumFromManifest -ManifestPath $manifest -Name "pc-windows-msvc.zip")

        Assert-Equal "Test-ArchiveChecksum accepts a matching archive" 0 `
            (Test-ArchiveChecksum -Path $payload -ManifestPath $manifest -Name "bharatcode-x86_64-pc-windows-msvc.zip")
        Assert-Equal "Test-ArchiveChecksum rejects a mismatched archive" 1 `
            (Test-ArchiveChecksum -Path $payload -ManifestPath $manifest -Name "bharatcode-x86_64-pc-windows-msvc-cuda.zip")
        Assert-Equal "Test-ArchiveChecksum reports a missing entry" 2 `
            (Test-ArchiveChecksum -Path $payload -ManifestPath $manifest -Name "bharatcode-not-published.zip")
    } finally {
        Remove-Item -Path $testDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host ""
    Write-Host "self-test: $script:passed passed, $script:failed failed"
    if ($script:failed -gt 0) { exit 1 }
    exit 0
}

# --- 1) Variables ---
$REPO = "arbazkhan971/bharatcode-cli"
$OUT_FILE = "bharatcode.exe"

# Set default bin directory if not specified
if (-not $env:BHARATCODE_BIN_DIR) {
    $env:BHARATCODE_BIN_DIR = Join-Path $env:USERPROFILE ".local\bin"
}

# Determine release type
$RELEASE = if ($env:CANARY -eq "true") { "true" } else { "false" }
$CONFIGURE = if ($env:CONFIGURE -eq "false") { "false" } else { "true" }
$ALLOW_UNVERIFIED = if ($env:BHARATCODE_ALLOW_UNVERIFIED -eq "true") { $true } else { $false }
$WINDOWS_VARIANT = if ($env:BHARATCODE_WINDOWS_VARIANT) { $env:BHARATCODE_WINDOWS_VARIANT.ToLowerInvariant() } else { "standard" }

# Determine release tag
if ($env:BHARATCODE_VERSION) {
    # Validate version format
    if ($env:BHARATCODE_VERSION -notmatch '^v?[0-9]+\.[0-9]+\.[0-9]+(-.*)?$') {
        Write-Error "Invalid version '$env:BHARATCODE_VERSION'. Expected: semver format vX.Y.Z, vX.Y.Z-suffix, or X.Y.Z"
        exit 1
    }
    # Ensure version starts with 'v'
    $RELEASE_TAG = if ($env:BHARATCODE_VERSION.StartsWith("v")) { $env:BHARATCODE_VERSION } else { "v$env:BHARATCODE_VERSION" }
} else {
    # Use canary or stable based on RELEASE variable
    $RELEASE_TAG = if ($RELEASE -eq "true") { "canary" } else { "stable" }
}

# --- 2) Detect Architecture ---
$ARCH = $env:PROCESSOR_ARCHITECTURE
if ($ARCH -eq "AMD64") {
    $ARCH = "x86_64"
} elseif ($ARCH -eq "ARM64") {
    Write-Error "Windows ARM64 is not currently supported."
    exit 1
} else {
    Write-Error "Unsupported architecture '$ARCH'. Only x86_64 is supported on Windows."
    exit 1
}

if ($WINDOWS_VARIANT -ne "standard" -and $WINDOWS_VARIANT -ne "cuda") {
    Write-Error "Unsupported BHARATCODE_WINDOWS_VARIANT '$WINDOWS_VARIANT'. Expected 'standard' or 'cuda'."
    exit 1
}

# --- 3) Build download URL ---
$FILE = if ($WINDOWS_VARIANT -eq "cuda") { "bharatcode-$ARCH-pc-windows-msvc-cuda.zip" } else { "bharatcode-$ARCH-pc-windows-msvc.zip" }
$DOWNLOAD_URL = "https://github.com/$REPO/releases/download/$RELEASE_TAG/$FILE"

$MANIFEST_URL = "https://github.com/$REPO/releases/download/$RELEASE_TAG/checksums.txt"

# --- 4) Create temporary directory ---
# The archive is downloaded, verified and extracted here, so an unverified archive is never
# left behind and never lands in the working directory.
$TMP_DIR = Join-Path $env:TEMP "bharatcode_install_$(Get-Random)"
try {
    New-Item -ItemType Directory -Path $TMP_DIR -Force | Out-Null
    Write-Host "Created temporary directory: $TMP_DIR" -ForegroundColor Yellow
} catch {
    Write-Error "Could not create temporary extraction directory: $TMP_DIR"
    exit 1
}

$ARCHIVE = Join-Path $TMP_DIR $FILE
$MANIFEST = Join-Path $TMP_DIR "checksums.txt"

# --- 5) Download the archive ---
Write-Host "Downloading $RELEASE_TAG release: $FILE..." -ForegroundColor Green
try {
    Invoke-WebRequest -Uri $DOWNLOAD_URL -OutFile $ARCHIVE -UseBasicParsing
    Write-Host "Download completed successfully." -ForegroundColor Green
} catch {
    Write-Error "Failed to download $DOWNLOAD_URL. Error: $($_.Exception.Message)"
    Remove-Item -Path $TMP_DIR -Recurse -Force -ErrorAction SilentlyContinue
    exit 1
}

# --- 5b) Verify the archive against the release checksum manifest ---
# Nothing below this point may extract or execute $ARCHIVE until it has been verified.
$MANIFEST_REQUIRED = -not $ALLOW_UNVERIFIED

Write-Host "Downloading checksums.txt for $RELEASE_TAG..." -ForegroundColor Green
$MANIFEST_OK = $true
try {
    Invoke-WebRequest -Uri $MANIFEST_URL -OutFile $MANIFEST -UseBasicParsing
} catch {
    $MANIFEST_OK = $false
}

if ($MANIFEST_OK) {
    switch (Test-ArchiveChecksum -Path $ARCHIVE -ManifestPath $MANIFEST -Name $FILE) {
        0 {
            Write-Host "Checksum verified: $FILE" -ForegroundColor Green
        }
        1 {
            # Always fatal. BHARATCODE_ALLOW_UNVERIFIED does not bypass a mismatch.
            Remove-Item -Path $TMP_DIR -Recurse -Force -ErrorAction SilentlyContinue
            Write-Error ("Checksum MISMATCH for $FILE. The download is corrupt or has been " +
                         "tampered with. Refusing to extract it.")
            exit 1
        }
        2 {
            if ($MANIFEST_REQUIRED) {
                Remove-Item -Path $TMP_DIR -Recurse -Force -ErrorAction SilentlyContinue
                Write-Error ("$FILE is not listed in checksums.txt for release '$RELEASE_TAG'. " +
                             "Refusing to extract an archive the release does not vouch for. " +
                             "Set `$env:BHARATCODE_ALLOW_UNVERIFIED='true' to install it anyway.")
                exit 1
            }
            Write-Warning "$FILE is not listed in checksums.txt; continuing unverified."
        }
    }
} elseif ($MANIFEST_REQUIRED) {
    Remove-Item -Path $TMP_DIR -Recurse -Force -ErrorAction SilentlyContinue
    Write-Error ("Release '$RELEASE_TAG' publishes no checksums.txt, so $FILE cannot be verified. " +
                 "Refusing to extract an unverified archive. Releases published before checksums.txt " +
                 "was introduced have no manifest; to install one of those, re-run with " +
                 "`$env:BHARATCODE_ALLOW_UNVERIFIED='true'.")
    exit 1
} else {
    Write-Warning "No checksums.txt for '$RELEASE_TAG'; continuing unverified."
}

# --- 6) Extract the verified archive ---
Write-Host "Extracting $FILE to temporary directory..." -ForegroundColor Green
try {
    Expand-Archive -Path $ARCHIVE -DestinationPath $TMP_DIR -Force
    Write-Host "Extraction completed successfully." -ForegroundColor Green
} catch {
    Write-Error "Failed to extract $FILE. Error: $($_.Exception.Message)"
    Remove-Item -Path $TMP_DIR -Recurse -Force -ErrorAction SilentlyContinue
    exit 1
}

# Keep the archive out of the extraction dir
Remove-Item -Path $ARCHIVE -Force

# --- 7) Determine extraction directory ---
$EXTRACT_DIR = $TMP_DIR
if (Test-Path (Join-Path $TMP_DIR "bharatcode-package")) {
    Write-Host "Found bharatcode-package subdirectory, using that as extraction directory" -ForegroundColor Yellow
    $EXTRACT_DIR = Join-Path $TMP_DIR "bharatcode-package"
}

# --- 8) Create bin directory if it doesn't exist ---
if (-not (Test-Path $env:BHARATCODE_BIN_DIR)) {
    Write-Host "Creating directory: $env:BHARATCODE_BIN_DIR" -ForegroundColor Yellow
    try {
        New-Item -ItemType Directory -Path $env:BHARATCODE_BIN_DIR -Force | Out-Null
    } catch {
        Write-Error "Could not create directory: $env:BHARATCODE_BIN_DIR"
        Remove-Item -Path $TMP_DIR -Recurse -Force -ErrorAction SilentlyContinue
        exit 1
    }
}

# --- 9) Install bharatcode binary ---
$SOURCE_BHARATCODE = Join-Path $EXTRACT_DIR "bharatcode.exe"
$DEST_BHARATCODE = Join-Path $env:BHARATCODE_BIN_DIR $OUT_FILE

if (Test-Path $SOURCE_BHARATCODE) {
    Write-Host "Moving bharatcode to $DEST_BHARATCODE" -ForegroundColor Green
    try {
        # Remove existing file if it exists to avoid conflicts
        if (Test-Path $DEST_BHARATCODE) {
            Remove-Item -Path $DEST_BHARATCODE -Force
        }
        Move-Item -Path $SOURCE_BHARATCODE -Destination $DEST_BHARATCODE -Force
    } catch {
        Write-Error "Failed to move bharatcode.exe to $DEST_BHARATCODE. Error: $($_.Exception.Message)"
        Remove-Item -Path $TMP_DIR -Recurse -Force -ErrorAction SilentlyContinue
        exit 1
    }
} else {
    Write-Error "bharatcode.exe not found in extracted files"
    Remove-Item -Path $TMP_DIR -Recurse -Force -ErrorAction SilentlyContinue
    exit 1
}

# --- 10) Copy Windows runtime DLLs if they exist ---
$DLL_FILES = Get-ChildItem -Path $EXTRACT_DIR -Filter "*.dll" -ErrorAction SilentlyContinue
foreach ($dll in $DLL_FILES) {
    $DEST_DLL = Join-Path $env:BHARATCODE_BIN_DIR $dll.Name
    Write-Host "Moving Windows runtime DLL: $($dll.Name)" -ForegroundColor Green
    try {
        # Remove existing file if it exists to avoid conflicts
        if (Test-Path $DEST_DLL) {
            Remove-Item -Path $DEST_DLL -Force
        }
        Move-Item -Path $dll.FullName -Destination $DEST_DLL -Force
    } catch {
        Write-Warning "Failed to move $($dll.Name): $($_.Exception.Message)"
    }
}

# --- 11) Clean up temporary directory ---
try {
    Remove-Item -Path $TMP_DIR -Recurse -Force
    Write-Host "Cleaned up temporary directory." -ForegroundColor Yellow
} catch {
    Write-Warning "Could not clean up temporary directory: $TMP_DIR"
}

# --- 12) Configure bharatcode (Optional) ---
if ($CONFIGURE -eq "true") {
    Write-Host ""
    Write-Host "Configuring bharatcode" -ForegroundColor Green
    Write-Host ""
    try {
        & $DEST_BHARATCODE configure
    } catch {
        Write-Warning "Failed to run bharatcode configure. You may need to run it manually later."
    }
} else {
    Write-Host "Skipping 'bharatcode configure', you may need to run this manually later" -ForegroundColor Yellow
}

# --- 13) Check PATH and give instructions if needed ---
$CURRENT_PATH = $env:PATH
if ($CURRENT_PATH -notlike "*$env:BHARATCODE_BIN_DIR*") {
    Write-Host ""
    Write-Host "Warning: bharatcode installed, but $env:BHARATCODE_BIN_DIR is not in your PATH." -ForegroundColor Yellow
    Write-Host "To add it to your PATH permanently, run the following command as Administrator:" -ForegroundColor Yellow
    Write-Host "    [Environment]::SetEnvironmentVariable('PATH', `$env:PATH + ';$env:BHARATCODE_BIN_DIR', 'Machine')" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Or add it to your user PATH (no admin required):" -ForegroundColor Yellow
    Write-Host "    [Environment]::SetEnvironmentVariable('PATH', `$env:PATH + ';$env:BHARATCODE_BIN_DIR', 'User')" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "For this session only, you can run:" -ForegroundColor Yellow
    Write-Host "    `$env:PATH += ';$env:BHARATCODE_BIN_DIR'" -ForegroundColor Cyan
    Write-Host ""
}

Write-Host "bharatcode CLI installation completed successfully!" -ForegroundColor Green
Write-Host "bharatcode is installed at: $DEST_BHARATCODE" -ForegroundColor Green
