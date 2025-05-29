#!/bin/bash

# Copyright 2024 The Aardvark Developers
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
 
set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${GREEN}🏗️  Aardvark Build Script${NC}"

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Initialize flags
CREATE_CLEAN=false
CREATE_APP_BUNDLE=false
CREATE_DMG=false
BUILD_TYPE="debug"

# Parse command line arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --clean) CREATE_CLEAN=true ;;
        --app-bundle) CREATE_APP_BUNDLE=true ;;
        --dmg) CREATE_DMG=true ;;
        --release) BUILD_TYPE="release" ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

# Ask before installing dependencies unless in CI
if [ -z "$CI" ]; then
    read -p "Please confirm you want to install dependencies, build and install Aardvark (y/n) " -n 1 -r
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

# Install dependencies
echo -e "${BLUE}📦 Installing/updating dependencies...${NC}"
if ! brew bundle; then
    echo -e "${YELLOW}⚠️  brew bundle failed, attempting to resolve Python linking conflicts...${NC}"
    
    # Try to force link python if it exists but isn't linked
    if brew list python@3.13 &> /dev/null; then
        echo -e "${YELLOW}🔗 Force linking Python...${NC}"
        brew link --overwrite python@3.13 || true
    fi
    
    # Retry brew bundle
    echo -e "${YELLOW}🔄 Retrying brew bundle...${NC}"
    if ! brew bundle; then
        echo -e "${RED}❌ brew bundle failed again. Please check the dependencies.${NC}"
        exit 1
    fi
fi

# Install and configure Rust nightly
echo -e "${YELLOW}🦀 Configuring Rust nightly...${NC}"
rustup toolchain install nightly
rustup default nightly

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
    meson setup builddir \
        --buildtype=$BUILD_TYPE \
        --prefix="$(pwd)/install" \
        -Dmacos_bundle=true
else
    echo -e "${YELLOW}📁 Using existing builddir (use './build.sh --clean' for clean build)${NC}"
fi

# Build the project
echo -e "${BLUE}🔨 Building Aardvark...${NC}"
meson compile -C builddir

# Install to local directory
echo -e "${BLUE}📦 Installing to local directory...${NC}"
meson install -C builddir

echo -e "${GREEN}✅ Build completed successfully!${NC}"

# Detect architecture for output naming
# Use TARGET_ARCH from CI environment if available, otherwise detect from system
if [ -n "$TARGET_ARCH" ]; then
    ARCH="$TARGET_ARCH"
    echo -e "${GREEN}📋 Built for: $ARCH (from TARGET_ARCH)${NC}"
else
    ARCH=$(uname -m)
    echo -e "${GREEN}📋 Built for: $ARCH${NC}"
fi

# Optional: Create macOS app bundle
if [ "$CREATE_APP_BUNDLE" = true ]; then
    echo -e "${BLUE}📱 Creating app bundle...${NC}"
    
    # Find the installed binary
    BINARY_PATH="install/bin/aardvark"
    if [ ! -f "$BINARY_PATH" ]; then
        echo -e "${RED}❌ Binary not found at $BINARY_PATH${NC}"
        exit 1
    fi
    
    # Create app bundle structure
    rm -rf Aardvark.app
    mkdir -p Aardvark.app/Contents/{MacOS,Resources,Frameworks}
    
    # Copy binary
    cp "$BINARY_PATH" Aardvark.app/Contents/MacOS/
    
    # Copy resources if they exist
    if [ -d "install/share" ]; then
        cp -r install/share Aardvark.app/Contents/Resources/
    fi
    
    # Create Info.plist
    cat > Aardvark.app/Contents/Info.plist << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>aardvark</string>
    <key>CFBundleIdentifier</key>
    <string>org.p2panda.aardvark</string>
    <key>CFBundleName</key>
    <string>Aardvark</string>
    <key>CFBundleDisplayName</key>
    <string>Aardvark</string>
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
    dylibbundler -od -b -x Aardvark.app/Contents/MacOS/aardvark \
        -d Aardvark.app/Contents/Frameworks/ \
        -p @executable_path/../Frameworks/ > /dev/null 2>&1 || echo -e "${YELLOW}⚠️  Some libraries may not be bundled${NC}"
    
    echo -e "${GREEN}✅ App bundle created: Aardvark.app${NC}"
    
    # Optional: Create DMG
    if [ "$CREATE_DMG" = true ]; then
        if command_exists create-dmg; then
            echo -e "${BLUE}💿 Creating DMG...${NC}"
            rm -f "aardvark-$ARCH.dmg"
            create-dmg \
                --volname "Aardvark" \
                --window-pos 200 120 \
                --window-size 600 400 \
                --icon-size 100 \
                --icon "Aardvark.app" 175 120 \
                --hide-extension "Aardvark.app" \
                --app-drop-link 425 120 \
                "aardvark-$ARCH.dmg" \
                "Aardvark.app"
            echo -e "${GREEN}✅ DMG created: aardvark-$ARCH.dmg${NC}"
        else
            echo -e "${YELLOW}⚠️  create-dmg command not found. Skipping DMG creation.${NC}"
        fi
    fi
fi

echo -e "  Direct: ${BLUE}./install/bin/aardvark${NC}"

if [ "$CREATE_APP_BUNDLE" = true ]; then
    if [ -d "Aardvark.app" ]; then
        echo -e "  App bundle: ${BLUE}open Aardvark.app${NC}"
    fi
fi

if [ "$CREATE_DMG" = true ]; then
    if [ -f "aardvark-$ARCH.dmg" ]; then
        echo -e "  DMG: ${BLUE}open \"aardvark-$ARCH.dmg\"${NC}"
    fi
fi