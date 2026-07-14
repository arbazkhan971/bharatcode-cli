#!/usr/bin/env bash
set -eu

##############################################################################
# bharatcode CLI Install Script
#
# This script downloads the latest stable 'bharatcode' CLI binary from GitHub releases
# and installs it to your system.
#
# Supported OS: macOS (darwin), Linux, Windows (MSYS2/Git Bash/WSL)
# Supported Architectures: x86_64, arm64
#
# Usage:
#   curl -fsSL https://github.com/arbazkhan971/bharatcode-cli/releases/download/stable/download_cli.sh | bash
#
# Environment variables:
#   BHARATCODE_BIN_DIR  - Directory to which bharatcode will be installed (default: $HOME/.local/bin)
#   BHARATCODE_VERSION  - Optional: specific version to install (e.g., "v1.0.25"). Overrides CANARY. Can be in the format vX.Y.Z, vX.Y.Z-suffix, or X.Y.Z
#   BHARATCODE_PROVIDER - Optional: provider for bharatcode
#   BHARATCODE_MODEL    - Optional: model for bharatcode
#   BHARATCODE_LINUX_VARIANT - Optional: Linux package variant to install (`standard`, `vulkan`, or `musl`)
#   BHARATCODE_WINDOWS_VARIANT - Optional: Windows package variant to install (`standard` or `cuda`)
#   CANARY         - Optional: if set to "true", downloads from canary release instead of stable
#   CONFIGURE      - Optional: if set to "false", disables running bharatcode configure interactively
#   BHARATCODE_ALLOW_UNVERIFIED - Optional: if "true", install even when the release publishes no
#                    checksums.txt. Only needed to install a pinned version older than the release
#                    that introduced the manifest. It never bypasses a checksum *mismatch*.
#   ** other provider specific environment variables (eg. DATABRICKS_HOST)
#
# Integrity:
#   Every release publishes checksums.txt, a sha256sum-format manifest covering every asset in
#   that release. This script downloads the manifest from the same release as the archive and
#   verifies the archive against it BEFORE extracting or executing anything from it.
#
#   Stable and pinned-version installs fail closed: a missing manifest, a missing entry for the
#   archive, or no available SHA-256 tool all abort the install unless the explicit legacy-release
#   escape hatch is set. A checksum mismatch is always fatal.
#
#   The manifest is served from the same origin as the archive, so this defends against corrupt,
#   truncated or substituted downloads and asset-name confusion -- not against a compromised
#   publisher. `bharatcode update` additionally verifies SLSA provenance via Sigstore.
##############################################################################

# --- 0) Integrity helpers ---
# Print the lowercase SHA-256 of a file, or return 1 if no hashing tool is available.
sha256_of_file() {
  _file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$_file" | awk '{print tolower($1)}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$_file" | awk '{print tolower($1)}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$_file" | awk '{print tolower($NF)}'
  else
    return 1
  fi
}

# Print the expected digest for exactly $2 from an sha256sum-format manifest, else nothing.
# Matching is on the whole filename: a request for "bharatcode.zip" must not match an entry
# for "bharatcode-x86_64-pc-windows-msvc.zip".
checksum_from_manifest() {
  _manifest="$1"
  _want="$2"
  [ -n "$_want" ] || return 0
  awk -v want="$_want" '
    NF >= 2 {
      file = $2
      sub(/^\*/, "", file)   # sha256sum binary-mode marker
      sub(/^\.\//, "", file)
      if (file == want && $1 ~ /^[0-9a-fA-F]{64}$/) {
        print tolower($1)
        exit
      }
    }' "$_manifest"
}

# Verify $1 against manifest $2 under the name $3.
#   0 = verified, 1 = MISMATCH, 2 = no entry in manifest, 3 = no SHA-256 tool available
verify_checksum() {
  _path="$1"
  _manifest="$2"
  _name="$3"
  _expected=$(checksum_from_manifest "$_manifest" "$_name")
  [ -n "$_expected" ] || return 2
  _actual=$(sha256_of_file "$_path") || return 3
  [ "$_expected" = "$_actual" ] || return 1
  return 0
}

# --- 0b) Self-test (BHARATCODE_SELF_TEST=true) ---
# Exercises the integrity helpers above without touching the network or the filesystem
# outside a temp dir. Run: BHARATCODE_SELF_TEST=true bash download_cli.sh
if [ "${BHARATCODE_SELF_TEST:-}" = "true" ]; then
  _t_pass=0
  _t_fail=0
  _check() { # _check <description> <expected> <actual>
    if [ "$2" = "$3" ]; then
      _t_pass=$((_t_pass + 1))
      echo "  ok   - $1"
    else
      _t_fail=$((_t_fail + 1))
      echo "  FAIL - $1 (expected '$2', got '$3')"
    fi
  }

  _dir=$(mktemp -d)
  trap 'rm -rf "$_dir"' EXIT

  printf 'hello world' > "$_dir/payload"
  # Known SHA-256 of "hello world" (no trailing newline).
  _hw=b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9

  _check "sha256_of_file matches known digest" "$_hw" "$(sha256_of_file "$_dir/payload")"

  cat > "$_dir/checksums.txt" <<EOF
$_hw  bharatcode-x86_64-unknown-linux-gnu.tar.bz2
$(printf '%064d' 0)  bharatcode-aarch64-apple-darwin.tar.bz2
${_hw}  *bharatcode-x86_64-pc-windows-msvc.zip
${_hw}  ./bharatcode-x86_64-unknown-linux-musl.tar.bz2
not-a-digest  bharatcode-bogus.tar.bz2
EOF

  _check "manifest lookup finds entry" \
    "$_hw" "$(checksum_from_manifest "$_dir/checksums.txt" bharatcode-x86_64-unknown-linux-gnu.tar.bz2)"
  _check "manifest lookup strips binary-mode marker" \
    "$_hw" "$(checksum_from_manifest "$_dir/checksums.txt" bharatcode-x86_64-pc-windows-msvc.zip)"
  _check "manifest lookup strips ./ prefix" \
    "$_hw" "$(checksum_from_manifest "$_dir/checksums.txt" bharatcode-x86_64-unknown-linux-musl.tar.bz2)"
  _check "manifest lookup ignores malformed digest" \
    "" "$(checksum_from_manifest "$_dir/checksums.txt" bharatcode-bogus.tar.bz2)"
  _check "manifest lookup misses absent asset" \
    "" "$(checksum_from_manifest "$_dir/checksums.txt" bharatcode-not-published.tar.bz2)"
  # A suffix/substring match here would let an attacker's asset satisfy the check.
  _check "manifest lookup does not substring-match" \
    "" "$(checksum_from_manifest "$_dir/checksums.txt" unknown-linux-gnu.tar.bz2)"

  verify_checksum "$_dir/payload" "$_dir/checksums.txt" bharatcode-x86_64-unknown-linux-gnu.tar.bz2
  _check "verify_checksum accepts a matching archive" "0" "$?"

  set +e
  verify_checksum "$_dir/payload" "$_dir/checksums.txt" bharatcode-aarch64-apple-darwin.tar.bz2
  _check "verify_checksum rejects a mismatched archive" "1" "$?"

  verify_checksum "$_dir/payload" "$_dir/checksums.txt" bharatcode-not-published.tar.bz2
  _check "verify_checksum reports a missing entry" "2" "$?"
  set -e

  echo ""
  echo "self-test: $_t_pass passed, $_t_fail failed"
  [ "$_t_fail" -eq 0 ] || exit 1
  exit 0
fi

# --- 1) Check for dependencies ---
# Check for curl
if ! command -v curl >/dev/null 2>&1; then
  echo "Error: 'curl' is required to download bharatcode. Please install curl and try again."
  exit 1
fi

# Check for tar or unzip (depending on OS)
if ! command -v tar >/dev/null 2>&1 && ! command -v unzip >/dev/null 2>&1; then
  echo "Error: Either 'tar' or 'unzip' is required to extract bharatcode. Please install one and try again."
  exit 1
fi

# Check for required extraction tools based on detected OS
if [ "${OS:-}" = "windows" ]; then
  # Windows uses PowerShell's built-in Expand-Archive - check if PowerShell is available
  if ! command -v powershell.exe >/dev/null 2>&1 && ! command -v pwsh >/dev/null 2>&1; then
    echo "Warning: PowerShell is recommended to extract Windows packages but was not found."
    echo "Falling back to unzip if available."
  fi
else
  if ! command -v tar >/dev/null 2>&1; then
    echo "Error: 'tar' is required to extract packages for ${OS:-unknown}. Please install tar and try again."
    exit 1
  fi
fi


# --- 2) Variables ---
REPO="arbazkhan971/bharatcode-cli"
OUT_FILE="bharatcode"

# Set default bin directory based on detected OS environment
if [[ "${WINDIR:-}" ]] || [[ "${windir:-}" ]] || [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "cygwin" ]]; then
    # Native Windows environments - use Windows user profile path
    DEFAULT_BIN_DIR="$USERPROFILE/bharatcode"
else
    # Linux, macOS, and WSL all use the same bin directory
    DEFAULT_BIN_DIR="$HOME/.local/bin"
fi

BHARATCODE_BIN_DIR="${BHARATCODE_BIN_DIR:-$DEFAULT_BIN_DIR}"
RELEASE="${CANARY:-false}"
CONFIGURE="${CONFIGURE:-true}"
BHARATCODE_ALLOW_UNVERIFIED="${BHARATCODE_ALLOW_UNVERIFIED:-false}"
BHARATCODE_LINUX_VARIANT="${BHARATCODE_LINUX_VARIANT:-}"
BHARATCODE_WINDOWS_VARIANT="${BHARATCODE_WINDOWS_VARIANT:-standard}"
if [ -n "${BHARATCODE_VERSION:-}" ]; then
  # Validate the version format
  if [[ ! "$BHARATCODE_VERSION" =~ ^v?[0-9]+\.[0-9]+\.[0-9]+(-.*)?$ ]]; then
    echo "[error]: invalid version '$BHARATCODE_VERSION'."
    echo "  expected: semver format vX.Y.Z, vX.Y.Z-suffix, or X.Y.Z"
    exit 1
  fi
  BHARATCODE_VERSION=$(echo "$BHARATCODE_VERSION" | sed 's/^v\{0,1\}/v/') # Ensure the version string is prefixed with 'v' if not already present
  RELEASE_TAG="$BHARATCODE_VERSION"
else
  # If BHARATCODE_VERSION is not set, fall back to existing behavior for backwards compatibility
  RELEASE_TAG="$([[ "$RELEASE" == "true" ]] && echo "canary" || echo "stable")"
fi

# --- 3) Detect OS/Architecture ---
# Allow explicit override for automation or when auto-detection is wrong:
#   INSTALL_OS=linux|windows|darwin
if [ -n "${INSTALL_OS:-}" ]; then
  case "${INSTALL_OS}" in
    linux|windows|darwin) OS="${INSTALL_OS}" ;;
    *) echo "[error]: unsupported INSTALL_OS='${INSTALL_OS}' (expected: linux|windows|darwin)"; exit 1 ;;
  esac
else
  # Better OS detection for Windows environments, with safer WSL handling.
  # If explicit Windows-like shells/variables are present (MSYS/Cygwin), treat as windows.
  if [[ "${WINDIR:-}" ]] || [[ "${windir:-}" ]] || [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "cygwin" ]]; then
    OS="windows"
  elif [[ -f "/proc/version" ]] && grep -q "Microsoft\|WSL" /proc/version 2>/dev/null; then
    # WSL is a Linux environment regardless of the current working directory.
    # The PWD (e.g. /mnt/c/) does not change the kernel — always install Linux.
    OS="linux"
  elif [[ "$OSTYPE" == "darwin"* ]]; then
    OS="darwin"
  elif [[ "$PWD" =~ ^/[a-zA-Z]/ ]] && [[ -d "/c" || -d "/d" || -d "/e" ]]; then
    # Check for Windows-style mount points (like in Git Bash)
    OS="windows"
  else
    # Fallback to uname for other systems
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
  fi
fi

ARCH=$(uname -m)

# Handle Windows environments (MSYS2, Git Bash, Cygwin, WSL)
case "$OS" in
  linux|darwin|windows) ;;
  mingw*|msys*|cygwin*)
    OS="windows"
    ;;
  *)
    echo "Error: Unsupported OS '$OS'. bharatcode currently supports Linux, macOS, and Windows."
    exit 1
    ;;
esac

case "$ARCH" in
  x86_64)
    ARCH="x86_64"
    ;;
  arm64|aarch64)
    # Some systems use 'arm64' and some 'aarch64' – standardize to 'aarch64'
    ARCH="aarch64"
    ;;
  *)
    echo "Error: Unsupported architecture '$ARCH'."
    exit 1
    ;;
esac

detect_linux_musl() {
  if [[ "$OSTYPE" == "linux-musl"* ]]; then
    return 0
  fi

  if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
    return 0
  fi

  return 1
}

if [ "$OS" = "linux" ] && [ -z "$BHARATCODE_LINUX_VARIANT" ]; then
  if detect_linux_musl; then
    BHARATCODE_LINUX_VARIANT="musl"
  else
    BHARATCODE_LINUX_VARIANT="standard"
  fi
elif [ -z "$BHARATCODE_LINUX_VARIANT" ]; then
  BHARATCODE_LINUX_VARIANT="standard"
fi

# Debug output (safely handle undefined variables)
echo "WINDIR: ${WINDIR:-<not set>}"
echo "OSTYPE: $OSTYPE"
echo "uname -s: $(uname -s)"
echo "uname -m: $(uname -m)"
echo "PWD: $PWD"

# Output the detected OS
echo "Detected OS: $OS with ARCH $ARCH"

# Build the filename and URL for the stable release
if [ "$OS" = "darwin" ]; then
  FILE="bharatcode-$ARCH-apple-darwin.tar.bz2"
  EXTRACT_CMD="tar"
elif [ "$OS" = "windows" ]; then
  case "$BHARATCODE_WINDOWS_VARIANT" in
    standard|cuda) ;;
    *)
      echo "Error: Unsupported BHARATCODE_WINDOWS_VARIANT '$BHARATCODE_WINDOWS_VARIANT'. Expected 'standard' or 'cuda'."
      exit 1
      ;;
  esac
  # Windows only supports x86_64 currently
  if [ "$ARCH" != "x86_64" ]; then
    echo "Error: Windows currently only supports x86_64 architecture."
    exit 1
  fi
  FILE="bharatcode-$ARCH-pc-windows-msvc.zip"
  if [ "$BHARATCODE_WINDOWS_VARIANT" = "cuda" ]; then
    FILE="bharatcode-$ARCH-pc-windows-msvc-cuda.zip"
  fi
  EXTRACT_CMD="unzip"
  OUT_FILE="bharatcode.exe"
else
  case "$BHARATCODE_LINUX_VARIANT" in
    standard|vulkan|musl) ;;
    *)
      echo "Error: Unsupported BHARATCODE_LINUX_VARIANT '$BHARATCODE_LINUX_VARIANT'. Expected 'standard', 'vulkan', or 'musl'."
      exit 1
      ;;
  esac
  FILE="bharatcode-$ARCH-unknown-linux-gnu.tar.bz2"
  if [ "$BHARATCODE_LINUX_VARIANT" = "vulkan" ]; then
    FILE="bharatcode-$ARCH-unknown-linux-gnu-vulkan.tar.bz2"
  elif [ "$BHARATCODE_LINUX_VARIANT" = "musl" ]; then
    FILE="bharatcode-$ARCH-unknown-linux-musl.tar.bz2"
  fi
  EXTRACT_CMD="tar"
fi

# Create a temporary directory. The archive is downloaded, verified and extracted here, so an
# unverified archive is never left behind and never lands in the working directory.
# mktemp (rather than a $RANDOM path) avoids colliding with, or being pre-created in, shared /tmp.
if ! TMP_DIR=$(mktemp -d 2>/dev/null); then
  echo "Error: Could not create temporary extraction directory"
  exit 1
fi
trap 'rm -rf "$TMP_DIR"' EXIT

ARCHIVE="$TMP_DIR/$FILE"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$RELEASE_TAG/$FILE"

# --- 4) Download 'bharatcode' archive ---
echo "Downloading $RELEASE_TAG release: $FILE..."
if ! curl -sLf "$DOWNLOAD_URL" --output "$ARCHIVE"; then
  # If the download fails, only fall back to latest stable when no version was specified and canary was not requested).
  if ! [ -n "${BHARATCODE_VERSION:-}" ] && [ "${CANARY:-false}" != "true" ]; then
    LATEST_TAG=$(curl -s https://api.github.com/repos/arbazkhan971/bharatcode-cli/releases/latest | \
      grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$LATEST_TAG" ]; then
      echo "Error: Failed to download $DOWNLOAD_URL and latest tag unavailable"
      exit 1
    fi

    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_TAG/$FILE"
    if curl -sLf "$DOWNLOAD_URL" --output "$ARCHIVE"; then
      # The manifest must come from the same release as the archive, not the tag we first tried.
      RELEASE_TAG="$LATEST_TAG"
    else
      echo "Error: Failed to download from fallback url $DOWNLOAD_URL using latest tag $LATEST_TAG"
      exit 1
    fi
  else
    echo "Error: Failed to download $DOWNLOAD_URL"
    exit 1
  fi
fi

# --- 4b) Verify the archive against the release checksum manifest ---
# Nothing below this point may extract or execute $ARCHIVE until it has been verified.
MANIFEST="$TMP_DIR/checksums.txt"
MANIFEST_URL="https://github.com/$REPO/releases/download/$RELEASE_TAG/checksums.txt"

if [ "$BHARATCODE_ALLOW_UNVERIFIED" = "true" ]; then
  MANIFEST_REQUIRED=false
else
  MANIFEST_REQUIRED=true
fi

echo "Downloading checksums.txt for $RELEASE_TAG..."
if curl -sLf "$MANIFEST_URL" --output "$MANIFEST"; then
  set +e
  verify_checksum "$ARCHIVE" "$MANIFEST" "$FILE"
  verify_status=$?
  set -e

  case "$verify_status" in
    0)
      echo "Checksum verified: $FILE"
      ;;
    1)
      # Always fatal. BHARATCODE_ALLOW_UNVERIFIED does not bypass a mismatch.
      echo "[error]: checksum MISMATCH for $FILE."
      echo "  expected: $(checksum_from_manifest "$MANIFEST" "$FILE")"
      echo "  actual:   $(sha256_of_file "$ARCHIVE" || echo '<unavailable>')"
      echo "  The download is corrupt or has been tampered with. Refusing to extract it."
      exit 1
      ;;
    2)
      if [ "$MANIFEST_REQUIRED" = "true" ]; then
        echo "[error]: $FILE is not listed in checksums.txt for release '$RELEASE_TAG'."
        echo "  Refusing to extract an archive the release does not vouch for."
        echo "  Set BHARATCODE_ALLOW_UNVERIFIED=true to install it anyway."
        exit 1
      fi
      echo "Warning: $FILE is not listed in checksums.txt; continuing unverified."
      ;;
    3)
      if [ "$MANIFEST_REQUIRED" = "true" ]; then
        echo "[error]: no SHA-256 tool found (need one of: sha256sum, shasum, openssl)."
        echo "  Cannot verify $FILE, so refusing to extract it."
        echo "  Install one, or set BHARATCODE_ALLOW_UNVERIFIED=true to install unverified."
        exit 1
      fi
      echo "Warning: no SHA-256 tool available; continuing unverified."
      ;;
  esac
elif [ "$MANIFEST_REQUIRED" = "true" ]; then
  echo "[error]: release '$RELEASE_TAG' publishes no checksums.txt, so $FILE cannot be verified."
  echo "  Refusing to extract an unverified archive."
  echo "  Releases before checksums.txt was introduced have no manifest; to install one of those,"
  echo "  re-run with BHARATCODE_ALLOW_UNVERIFIED=true."
  exit 1
else
  echo "Warning: no checksums.txt for '$RELEASE_TAG'; continuing unverified."
fi

# --- 4c) Extract the verified archive ---
echo "Extracting $FILE to temporary directory..."
set +e  # Disable immediate exit on error

if [ "$EXTRACT_CMD" = "tar" ]; then
  tar -xjf "$ARCHIVE" -C "$TMP_DIR" 2> "$TMP_DIR/tar_error.log"
  extract_exit_code=$?

  # Check for tar errors
  if [ $extract_exit_code -ne 0 ]; then
    if grep -iEq "missing.*bzip2|bzip2.*missing|bzip2.*No such file|No such file.*bzip2" "$TMP_DIR/tar_error.log"; then
      echo "Error: Failed to extract $FILE. 'bzip2' is required but not installed. See details below:"
    else
      echo "Error: Failed to extract $FILE. See details below:"
    fi
    cat "$TMP_DIR/tar_error.log"
    exit 1
  fi
else
  # Use unzip for Windows
  unzip -q "$ARCHIVE" -d "$TMP_DIR" 2> "$TMP_DIR/unzip_error.log"
  extract_exit_code=$?

  # Check for unzip errors
  if [ $extract_exit_code -ne 0 ]; then
    echo "Error: Failed to extract $FILE. See details below:"
    cat "$TMP_DIR/unzip_error.log"
    exit 1
  fi
fi

set -e  # Re-enable immediate exit on error

rm -f "$ARCHIVE" # keep the archive out of the extraction dir

# Determine the extraction directory (handle subdirectory in Windows packages)
# Windows releases may contain files in a 'bharatcode-package' subdirectory
EXTRACT_DIR="$TMP_DIR"
if [ "$OS" = "windows" ] && [ -d "$TMP_DIR/bharatcode-package" ]; then
  echo "Found bharatcode-package subdirectory, using that as extraction directory"
  EXTRACT_DIR="$TMP_DIR/bharatcode-package"
fi

# Make binary executable
if [ "$OS" = "windows" ]; then
  chmod +x "$EXTRACT_DIR/bharatcode.exe"
else
  chmod +x "$EXTRACT_DIR/bharatcode"
fi

# --- 5) Install to $BHARATCODE_BIN_DIR ---
if [ ! -d "$BHARATCODE_BIN_DIR" ]; then
  echo "Creating directory: $BHARATCODE_BIN_DIR"
  mkdir -p "$BHARATCODE_BIN_DIR"
fi

echo "Moving bharatcode to $BHARATCODE_BIN_DIR/$OUT_FILE"
if [ "$OS" = "windows" ]; then
  mv "$EXTRACT_DIR/bharatcode.exe" "$BHARATCODE_BIN_DIR/$OUT_FILE"
else
  # On Linux, if the target binary is currently running, writing to it fails
  # with ETXTBSY ("Text file busy"). Rename the old binary out of the way
  # first, then move the new one in. If the move fails, restore the old binary
  # so the user is never left without an executable.
  if [ -f "$BHARATCODE_BIN_DIR/$OUT_FILE" ]; then
    mv "$BHARATCODE_BIN_DIR/$OUT_FILE" "$BHARATCODE_BIN_DIR/$OUT_FILE.old"
    if ! mv "$EXTRACT_DIR/bharatcode" "$BHARATCODE_BIN_DIR/$OUT_FILE"; then
      echo "Error: failed to install new binary, restoring previous version"
      mv "$BHARATCODE_BIN_DIR/$OUT_FILE.old" "$BHARATCODE_BIN_DIR/$OUT_FILE"
      exit 1
    fi
    rm -f "$BHARATCODE_BIN_DIR/$OUT_FILE.old"
  else
    mv "$EXTRACT_DIR/bharatcode" "$BHARATCODE_BIN_DIR/$OUT_FILE"
  fi
fi

# Copy Windows runtime DLLs if they exist
if [ "$OS" = "windows" ]; then
  for dll in "$EXTRACT_DIR"/*.dll; do
    if [ -f "$dll" ]; then
      echo "Moving Windows runtime DLL: $(basename "$dll")"
      mv "$dll" "$BHARATCODE_BIN_DIR/"
    fi
  done
fi

# skip configuration for non-interactive installs e.g. automation, docker
if [ "$CONFIGURE" = true ]; then
  # --- 6) Configure bharatcode (Optional) ---
  echo ""
  echo "Configuring bharatcode"
  echo ""
  if [ -t 0 ]; then
    "$BHARATCODE_BIN_DIR/$OUT_FILE" configure
  elif [ -r /dev/tty ]; then
    "$BHARATCODE_BIN_DIR/$OUT_FILE" configure < /dev/tty
  else
    echo "Non-interactive shell detected (e.g. 'curl ... | bash')."
    echo "Skipping 'bharatcode configure' — please run it manually after installation:"
    echo "    $BHARATCODE_BIN_DIR/$OUT_FILE configure"
  fi
else
  echo "Skipping 'bharatcode configure', you may need to run this manually later"
fi



# --- 7) Check PATH and give instructions if needed ---
if [[ ":$PATH:" != *":$BHARATCODE_BIN_DIR:"* ]]; then
  echo ""
  echo "Warning: bharatcode installed, but $BHARATCODE_BIN_DIR is not in your PATH."

  if [ "$OS" = "windows" ]; then
    echo "To add bharatcode to your PATH in PowerShell:"
    echo ""
    echo "# Add to your PowerShell profile"
    echo '$profilePath = $PROFILE'
    echo 'if (!(Test-Path $profilePath)) { New-Item -Path $profilePath -ItemType File -Force }'
    echo 'Add-Content -Path $profilePath -Value ''$env:PATH = "$env:USERPROFILE\.local\bin;$env:PATH"'''
    echo "# Reload profile or restart PowerShell"
    echo '. $PROFILE'
    echo ""
    echo "Alternatively, you can run:"
    echo "    bharatcode configure"
    echo "or rerun this install script after updating your PATH."
  else
    SHELL_NAME=$(basename "$SHELL")

    echo ""
    echo "The \$BHARATCODE_BIN_DIR is not in your PATH."

    if [ "$CONFIGURE" = true ]; then
      echo "What would you like to do?"
      echo "1) Add it for me"
      echo "2) I'll add it myself, show instructions"

      # Check whether stdin is a terminal. If it is not (for example, if
      # this script has been piped into bash), we need to explicitly read user's
      # choice from /dev/tty.
      if [ -t 0 ]; then # terminal
        read -p "Enter choice [1/2]: " choice
      elif [ -r /dev/tty ]; then # not a terminal, but /dev/tty is available
        read -p "Enter choice [1/2]: " choice < /dev/tty
      else # non-interactive environment without /dev/tty
        echo "Non-interactive environment detected without /dev/tty; defaulting to option 2 (show instructions)."
        choice=2
      fi

      case "$choice" in
      1)
        RC_FILE="$HOME/.${SHELL_NAME}rc"
        echo "Adding \$BHARATCODE_BIN_DIR to $RC_FILE..."
        echo "export PATH=\"$BHARATCODE_BIN_DIR:\$PATH\"" >> "$RC_FILE"
        echo "Done! Reload your shell or run 'source $RC_FILE' to apply changes."
        ;;
      2)
        echo ""
        echo "Add it to your PATH by editing ~/.${SHELL_NAME}rc or similar:"
        echo "    export PATH=\"$BHARATCODE_BIN_DIR:\$PATH\""
        echo "Then reload your shell (e.g. 'source ~/.${SHELL_NAME}rc') to apply changes."
        ;;
      *)
        echo "Invalid choice. Please add \$BHARATCODE_BIN_DIR to your PATH manually."
        ;;
      esac
    else
      echo ""
      echo "Configure disabled. Please add \$BHARATCODE_BIN_DIR to your PATH manually."
    fi

  fi

  echo ""
fi
