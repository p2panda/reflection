#!/bin/bash

# Copyright 2026 The Reflection Developers
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

# Checks that the app in a DMG works on a Mac without Homebrew, and when it
# isn't installed in /Applications. Doesn't touch the system or user data.

set -u

if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <dmg>"
    exit 2
fi

DMG=$1
WORK_DIR=$(mktemp -d)
APP="$WORK_DIR/not applications/Reflection.app"
LOG="$WORK_DIR/reflection.log"
MOUNT_POINT="$WORK_DIR/mnt"
FAILED=0

cleanup() {
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

fail() {
    echo "FAILED: $1"
    FAILED=1
}

# Install from the DMG, the way a user does it
mkdir -p "$MOUNT_POINT" "$(dirname "$APP")" "$WORK_DIR/home"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT_POINT" "$DMG" || exit 1
cp -R "$MOUNT_POINT/Reflection.app" "$APP"
hdiutil detach -quiet "$MOUNT_POINT" || exit 1

echo "Checking bundle contents..."
for path in \
    "Contents/Info.plist" \
    "Contents/Resources/reflection/resources.gresource" \
    "Contents/Resources/glib-2.0/schemas/gschemas.compiled" \
    "Contents/Resources/gtksourceview-5/language-specs/language2.rng" \
    "Contents/Resources/gdk-pixbuf/loaders.cache" \
    "Contents/Resources/icons/Adwaita/index.theme" \
    "Contents/Resources/icons/hicolor/index.theme" \
    "Contents/PlugIns/gdk-pixbuf/libpixbufloader_svg.dylib" \
    "Contents/PlugIns/enchant/lib/enchant-2/enchant_applespell.so"; do
    [ -e "$APP/$path" ] || fail "$path is missing"
done

# gdk-pixbuf-query-loaders writes an empty cache if it can't load a module
if ! grep -q '^"@executable_path/../PlugIns/gdk-pixbuf/libpixbufloader_svg.dylib"$' \
    "$APP/Contents/Resources/gdk-pixbuf/loaders.cache"; then
    fail "loaders.cache doesn't contain the SVG loader"
fi

for schema in EmojiChooser FileChooser ColorChooser; do
    gsettings --schemadir "$APP/Contents/Resources/glib-2.0/schemas" list-schemas \
        | grep -q "org.gtk.gtk4.Settings.$schema" || fail "GSettings schema $schema is missing"
done

echo "Checking binaries..."
MINIMUM_VERSION=$(/usr/libexec/PlistBuddy -c "Print :LSMinimumSystemVersion" "$APP/Contents/Info.plist")
while IFS= read -r -d '' binary; do
    file -b "$binary" | grep -q "Mach-O" || continue
    name=${binary#"$APP/"}

    if otool -L "$binary" | tail -n +2 | grep -E "/opt/homebrew|/usr/local|@@HOMEBREW|@rpath"; then
        fail "$name links outside of the bundle"
    fi

    required_version=$(vtool -show-build "$binary" | awk '/minos/ { print $2 }' | head -n 1)
    newest_version=$(printf '%s\n%s\n' "$MINIMUM_VERSION" "$required_version" | sort -V | tail -n 1)
    if [ "$newest_version" != "$MINIMUM_VERSION" ]; then
        fail "$name requires macOS $required_version, but the bundle claims to support $MINIMUM_VERSION"
    fi
done < <(find "$APP/Contents" -type f -print0)

codesign --verify --deep --strict "$APP" || fail "Code signature of the bundle is invalid"

echo "Running without access to Homebrew or the keychain..."
SANDBOX_PROFILE='(version 1)
(allow default)
(deny file-read* file-test-existence (subpath "/opt/homebrew") (subpath "/usr/local"))
(deny mach-lookup (global-name "com.apple.SecurityServer") (global-name "com.apple.securityd.xpc"))'

HOME="$WORK_DIR/home" \
    RUST_BACKTRACE=full \
    RUST_LOG=debug \
    sandbox-exec -p "$SANDBOX_PROFILE" "$APP/Contents/MacOS/reflection" > "$LOG" 2>&1 &
PID=$!
sleep 5

if kill -0 "$PID" 2>/dev/null; then
    # Modules are only used on demand, so check the ones needed for every document
    lsof -p "$PID" | grep -q "PlugIns/enchant/lib/enchant-2/enchant_applespell.so" \
        || fail "The spell checking provider wasn't loaded"
    kill "$PID"
    wait "$PID" 2>/dev/null
else
    wait "$PID"
    fail "The app exited early with status $?"
fi

if grep -E "panicked at|Library not loaded|Settings schema .* is not installed|GLib-GIO-ERROR" "$LOG"; then
    fail "The log contains fatal errors"
fi
if grep -E "/opt/homebrew|/usr/local" "$LOG"; then
    fail "Tried to use files from outside of the bundle"
fi

cat "$LOG"

if [ "$FAILED" -ne 0 ]; then
    echo "Smoke test failed"
    exit 1
fi
echo "Smoke test passed"
