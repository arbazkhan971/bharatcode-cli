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
#   curl -fsSL https://github.com/aaif-bharatcode/bharatcode/releases/download/stable/download_cli.sh | bash
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
#   ** other provider specific environment variables (eg. DATABRICKS_HOST)
##############################################################################

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
REPO="aaif-bharatcode/bharatcode"
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

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$RELEASE_TAG/$FILE"

# --- 4) Download & extract 'bharatcode' binary ---
echo "Downloading $RELEASE_TAG release: $FILE..."
if ! curl -sLf "$DOWNLOAD_URL" --output "$FILE"; then
  # If the download fails, only fall back to latest stable when no version was specified and canary was not requested).
  if ! [ -n "${BHARATCODE_VERSION:-}" ] && [ "${CANARY:-false}" != "true" ]; then
    LATEST_TAG=$(curl -s https://api.github.com/repos/aaif-bharatcode/bharatcode/releases/latest | \
      grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$LATEST_TAG" ]; then
      echo "Error: Failed to download $DOWNLOAD_URL and latest tag unavailable"
      exit 1
    fi

    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_TAG/$FILE"
    if curl -sLf "$DOWNLOAD_URL" --output "$FILE"; then
      # Fallback succeeded
      :
    else
      echo "Error: Failed to download from fallback url $DOWNLOAD_URL using latest tag $LATEST_TAG"
      exit 1
    fi
  else
    echo "Error: Failed to download $DOWNLOAD_URL"
    exit 1
  fi
fi

# Create a temporary directory for extraction
TMP_DIR="/tmp/bharatcode_install_$RANDOM"
if ! mkdir -p "$TMP_DIR"; then
  echo "Error: Could not create temporary extraction directory"
  exit 1
fi
# Clean up temporary directory
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Extracting $FILE to temporary directory..."
set +e  # Disable immediate exit on error

if [ "$EXTRACT_CMD" = "tar" ]; then
  tar -xjf "$FILE" -C "$TMP_DIR" 2> tar_error.log
  extract_exit_code=$?

  # Check for tar errors
  if [ $extract_exit_code -ne 0 ]; then
    if grep -iEq "missing.*bzip2|bzip2.*missing|bzip2.*No such file|No such file.*bzip2" tar_error.log; then
      echo "Error: Failed to extract $FILE. 'bzip2' is required but not installed. See details below:"
    else
      echo "Error: Failed to extract $FILE. See details below:"
    fi
    cat tar_error.log
    rm tar_error.log
    exit 1
  fi
  rm tar_error.log
else
  # Use unzip for Windows
  unzip -q "$FILE" -d "$TMP_DIR" 2> unzip_error.log
  extract_exit_code=$?

  # Check for unzip errors
  if [ $extract_exit_code -ne 0 ]; then
    echo "Error: Failed to extract $FILE. See details below:"
    cat unzip_error.log
    rm unzip_error.log
    exit 1
  fi
  rm unzip_error.log
fi

set -e  # Re-enable immediate exit on error

rm "$FILE" # clean up the downloaded archive

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
