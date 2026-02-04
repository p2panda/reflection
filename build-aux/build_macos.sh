#!/bin/bash

# Copyright 2024 The Reflection Developers
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
#
# SPDX-License-Identifier: GPL-3.0-or-later

# FIXME: Do as much as possible in meson see: https://mesonbuild.com/Creating-OSX-packages.html

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${GREEN}🏗️ Reflection Build Script${NC}"

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Initialize flags
CREATE_APP_BUNDLE=false
CREATE_DMG=false
CREATE_CLEAN=false
BUILD_TYPE="debug"
ARCH=$(uname -m)
# Parse command line arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --help|-h)
            echo "Usage: $0 [options]"
            echo "Options:"
            echo "  --app-bundle   Create macOS .app bundle"
            echo "  --dmg          Create DMG installer"
            echo "  --clean        Clean build directory before building"
            echo "  --release      Build in release mode"
            echo "  --arch ARCH    Target architecture (default: $(uname -m))"
            echo "  --help, -h     Show this help message"
            exit 0
            ;;
        --app-bundle) CREATE_APP_BUNDLE=true ;;
        --dmg) CREATE_DMG=true ;;
        --clean) CREATE_CLEAN=true ;;
        --release) BUILD_TYPE="release" ;;
        --arch) ARCH="$2"; shift ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

# Determine if we need to cross-compile based on target architecture
if [ "$ARCH" != "$(uname -m)" ]; then
    CROSS_COMPILE=true
    echo -e "${YELLOW}👽 Cross-compiling for $ARCH architecture${NC}"
else
    CROSS_COMPILE=false
fi


# Ask before installing dependencies unless in CI
if [ -z "$CI" ]; then
    read -p "Please confirm you want to install dependencies, build and install Reflection (y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo -e "${RED}❌ Aborting...${NC}"
        exit 1
    fi
fi

# Check if Homebrew is installed
if ! command_exists brew; then
    echo -e "${RED}❌ Homebrew not found. Please install it first.${NC}"
    exit 1
fi

if [ "$CROSS_COMPILE" = true ]; then
    BREW_ARCH_CMD="arch -$ARCH brew"
else
    BREW_ARCH_CMD="brew"
fi

# Install dependencies
echo -e "${BLUE}📦 Installing/updating dependencies...${NC}"
if ! $BREW_ARCH_CMD bundle --file=$(pwd)/build-aux/Brewfile; then
    echo -e "${YELLOW}⚠️  brew bundle failed, attempting to resolve Python linking conflicts...${NC}"

    # Try to force link python if it exists but isn't linked
    if brew list python@3.13 &> /dev/null; then
        echo -e "${YELLOW}🔗 Force linking Python...${NC}"
        $BREW_ARCH_CMD link --overwrite python@3.13 || true
    fi

    # Retry brew bundle
    echo -e "${YELLOW}🔄 Retrying brew bundle...${NC}"
    if ! $BREW_ARCH_CMD bundle --file=$(pwd)/build-aux/Brewfile; then
        echo -e "${RED}❌ brew bundle failed again. Please check the dependencies.${NC}"
        exit 1
    fi
fi

# Set up environment for system libraries
export PKG_CONFIG_PATH="$HOMEBREW_PREFIX/lib/pkgconfig:$PKG_CONFIG_PATH"
export GETTEXT_SYSTEM=1
export GETTEXT_DIR="$HOMEBREW_PREFIX"
export GETTEXT_LIB_DIR="$HOMEBREW_PREFIX/lib"
export GETTEXT_INCLUDE_DIR="$HOMEBREW_PREFIX/include"

echo -e "${BLUE}⚙️  Configuring build with Meson...${NC}"

# Only remove builddir if explicitly requested
if [ "$CREATE_CLEAN" = true ]; then
    echo -e "${YELLOW}🧹 Clean build requested, removing builddir...${NC}"
    rm -rf builddir
fi

if [ ! -d "builddir" ]; then
    BUILD_ARGS=(
        --buildtype=$BUILD_TYPE \
        --prefix="$(pwd)/Reflection.app" \
        --bindir="Contents/MacOS" \
        --datadir="Contents/Resources" \
        --localedir="Contents/Resources/locale"
    )
    if [ "$CROSS_COMPILE" = true ]; then
        BUILD_ARGS+=(--cross-file="$(pwd)/build-aux/$ARCH-darwin-cross.txt")
    fi
    meson setup builddir "${BUILD_ARGS[@]}"
else
    echo -e "${YELLOW}📁 Using existing builddir (use --clean for clean build)${NC}"
fi

# Build the project
echo -e "${BLUE}🔨 Building Reflection...${NC}"
meson compile -C builddir

# Install to local directory
echo -e "${BLUE}📦 Installing to local directory...${NC}"
meson install -C builddir

echo -e "${GREEN}✅ Build completed successfully!${NC}"

echo -e "${GREEN}📋 Built for: $ARCH${NC}"

# Optional: Create macOS app bundle
if [ "$CREATE_APP_BUNDLE" = true ]; then
    echo -e "${BLUE}📱 Creating app bundle...${NC}"

    # Create app bundle structure
    mkdir -p Reflection.app/Contents/Frameworks

    # FIXME: We should add the Info.plist file using meson
    # Create Info.plist
    cat > Reflection.app/Contents/Info.plist << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>reflection</string>
    <key>CFBundleIdentifier</key>
    <string>cx.modal.Reflection</string>
    <key>CFBundleName</key>
    <string>Reflection</string>
    <key>CFBundleDisplayName</key>
    <string>Reflection</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>MacOSX</string>
    </array>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

    # Bundle dependencies
    echo -e "${BLUE}🔗 Bundling dependencies...${NC}"
    dylibbundler -od -b -x Reflection.app/Contents/MacOS/reflection \
        -d Reflection.app/Contents/Frameworks/ \
        -p @executable_path/../Frameworks/ > /dev/null 2>&1 || echo -e "${YELLOW}⚠️  Some libraries may not be bundled${NC}"

    echo -e "${GREEN}✅ App bundle created: Reflection.app${NC}"

    # Optional: Create DMG
    if [ "$CREATE_DMG" = true ]; then
        if command_exists create-dmg; then
            echo -e "${BLUE}💿 Creating DMG...${NC}"
            rm -f "reflection-$ARCH.dmg"
            create-dmg \
                --volname "Reflection" \
                --window-pos 200 120 \
                --window-size 600 400 \
                --icon-size 100 \
                --icon "Reflection.app" 175 120 \
                --hide-extension "Reflection.app" \
                --app-drop-link 425 120 \
                "reflection-$ARCH.dmg" \
                "Reflection.app"
            echo -e "${GREEN}✅ DMG created: reflection-$ARCH.dmg${NC}"
        else
            echo -e "${YELLOW}⚠️  create-dmg command not found. Skipping DMG creation.${NC}"
        fi
    fi
fi

echo -e "  Direct: ${BLUE}./Reflection.app/bin/reflection${NC}"

if [ "$CREATE_APP_BUNDLE" = true ]; then
    if [ -d "Reflection.app" ]; then
        echo -e "  App bundle: ${BLUE}open Reflection.app${NC}"
    fi
fi

if [ "$CREATE_DMG" = true ]; then
    if [ -f "reflection-$ARCH.dmg" ]; then
        echo -e "  DMG: ${BLUE}open \"reflection-$ARCH.dmg\"${NC}"
    fi
fi