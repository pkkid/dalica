#!/usr/bin/env bash
# The AppImage's AppRun (packaging/linux/AppRun.sh): its desktop integration writes and refreshes
# the user's .desktop entry and icons, does nothing twice, honours the opt-out, an external
# integrator and an unwritable data folder, and always starts the app with its arguments.
#
# Runs in a second with no build; package.sh runs it before making the AppImage.
#
# Adapted from PhotoCraft packaging/linux/apprun-test.sh @ dc35ce76 (MIT OR Apache-2.0), with
# Dalica's names and an Exec that keeps its file argument (`%f`, opened in the photo viewer).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP_ID=org.dalica.Dalica

TMP="$(mktemp -d)"
trap 'chmod -R u+w "$TMP" 2>/dev/null; rm -rf "$TMP"' EXIT

# A fake AppDir: the real AppRun, desktop entry and icons, and a stub binary.
APPDIR="$TMP/Dalica.AppDir"
mkdir -p "$APPDIR/usr/bin"
install -Dm755 "$HERE/AppRun.sh" "$APPDIR/AppRun"
install -Dm644 "$HERE/$APP_ID.desktop" "$APPDIR/usr/share/applications/$APP_ID.desktop"
mkdir -p "$APPDIR/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$APPDIR/usr/share/icons/"
# shellcheck disable=SC2016 # the stub's own $*, expanded when it runs
printf '#!/bin/sh\necho "stub: $*"\n' >"$APPDIR/usr/bin/dalica"
chmod +x "$APPDIR/usr/bin/dalica"

APPIMAGE="$TMP/dalica test.AppImage" # a space on purpose: Exec must stay quoted
: >"$APPIMAGE"
export APPIMAGE
export HOME="$TMP/home"
export XDG_DATA_HOME="$HOME/.local/share"
unset DESKTOPINTEGRATION DALICA_NO_DESKTOP_INTEGRATION
mkdir -p "$HOME"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# 1. The launch works and passes the arguments on.
out="$("$APPDIR/AppRun" --demo --library x)" || fail "AppRun exited non-zero"
[ "$out" = "stub: --demo --library x" ] || fail "arguments not passed on: $out"

DESKTOP="$XDG_DATA_HOME/applications/$APP_ID.desktop"
[ -f "$DESKTOP" ] || fail "desktop entry not installed"

# 2. Exec points at the AppImage (quoted, the space kept) with the packaged file argument, TryExec
#    is gone, the rest is as packaged.
expected_exec="Exec=\"$(readlink -f "$APPIMAGE")\" %f"
grep -Fxq "$expected_exec" "$DESKTOP" || fail "Exec not rewritten: $(grep '^Exec=' "$DESKTOP")"
if grep -q '^TryExec=' "$DESKTOP"; then fail "TryExec kept, the entry would be hidden"; fi
grep -Fxq "Icon=$APP_ID" "$DESKTOP" || fail "Icon line lost"
grep -Fxq "StartupWMClass=$APP_ID" "$DESKTOP" || fail "StartupWMClass lost"
grep -Fxq "X-AppImage-Integrate=false" "$DESKTOP" || fail "X-AppImage-Integrate=false lost"
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$DESKTOP" || fail "the installed desktop entry is invalid"
fi

# 3. The icons are where the entry's Icon= finds them.
for f in "16x16/apps/$APP_ID.png" "256x256/apps/$APP_ID.png" "512x512/apps/$APP_ID.png" "scalable/apps/$APP_ID.svg"; do
  [ -f "$XDG_DATA_HOME/icons/hicolor/$f" ] || fail "icon missing: $f"
done

# 4. A second launch rewrites nothing (same inode).
before="$(stat -c %i "$DESKTOP")"
out="$("$APPDIR/AppRun")" || fail "second launch exited non-zero"
[ "$out" = "stub: " ] || fail "second launch output: $out"
after="$(stat -c %i "$DESKTOP")"
[ "$before" = "$after" ] || fail "desktop entry rewritten although unchanged"

# 5. Moving the AppImage refreshes Exec.
mv "$APPIMAGE" "$TMP/moved.AppImage"
export APPIMAGE="$TMP/moved.AppImage"
"$APPDIR/AppRun" >/dev/null || fail "launch after a move exited non-zero"
grep -Fxq "Exec=\"$(readlink -f "$APPIMAGE")\" %f" "$DESKTOP" || fail "Exec not refreshed after a move"

# 6. An external integrator's entry for this AppImage: ours goes, the icons stay.
ext_desktop="$XDG_DATA_HOME/applications/appimagekit-dalica.desktop"
printf '[Desktop Entry]\nType=Application\nName=Dalica\nExec="%s"\nIcon=%s\n' "$(readlink -f "$APPIMAGE")" "$APP_ID" >"$ext_desktop"
"$APPDIR/AppRun" >/dev/null || fail "launch beside an external entry exited non-zero"
[ ! -e "$DESKTOP" ] || fail "a second entry kept beside the external integrator's"
[ -f "$ext_desktop" ] || fail "the external integrator's entry was removed"
[ -f "$XDG_DATA_HOME/icons/hicolor/256x256/apps/$APP_ID.png" ] || fail "icons missing beside an external integrator"
rm -f "$ext_desktop"

# 7. DESKTOPINTEGRATION (set by integrators) means no entry of ours.
rm -rf "$XDG_DATA_HOME"
DESKTOPINTEGRATION=AppImageLauncher "$APPDIR/AppRun" >/dev/null || fail "DESKTOPINTEGRATION launch exited non-zero"
[ ! -e "$DESKTOP" ] || fail "DESKTOPINTEGRATION still wrote an entry"
[ -f "$XDG_DATA_HOME/icons/hicolor/256x256/apps/$APP_ID.png" ] || fail "icons not installed under DESKTOPINTEGRATION"

# 8. The opt-out installs nothing.
rm -rf "$XDG_DATA_HOME"
DALICA_NO_DESKTOP_INTEGRATION=1 "$APPDIR/AppRun" >/dev/null || fail "opt-out launch exited non-zero"
[ ! -e "$XDG_DATA_HOME" ] || fail "the opt-out still integrated"

# 9. An unwritable data folder never blocks the launch.
mkdir -p "$TMP/ro" && chmod a-w "$TMP/ro"
out="$(XDG_DATA_HOME="$TMP/ro/data" "$APPDIR/AppRun" --demo)" || fail "launch blocked by an unwritable data folder"
[ "$out" = "stub: --demo" ] || fail "unwritable data folder: $out"
chmod u+w "$TMP/ro"

# 10. No $APPIMAGE (an extracted AppDir): the launch only, nothing installed.
rm -rf "$XDG_DATA_HOME"
env -u APPIMAGE "$APPDIR/AppRun" >/dev/null || fail "extracted-AppDir launch exited non-zero"
[ ! -e "$XDG_DATA_HOME" ] || fail "integrated without \$APPIMAGE"

# 11. A path that Exec would need to escape (here a `$`): the launch only, nothing installed.
rm -rf "$XDG_DATA_HOME"
odd="$TMP/odd\$(id).AppImage"
: >"$odd"
out="$(APPIMAGE="$odd" "$APPDIR/AppRun" x)" || fail "launch with an odd path exited non-zero"
[ "$out" = "stub: x" ] || fail "odd path: $out"
[ ! -e "$XDG_DATA_HOME" ] || fail "integrated an AppImage path that needs escaping"

echo "AppRun desktop integration ok"
