#!/bin/sh
# The AppImage's entry point, installed as `AppRun` at the top of the AppDir by package.sh.
#
# Adapted from PhotoCraft packaging/linux/AppRun @ dc35ce76 (MIT OR Apache-2.0). Changed for
# Dalica: its names, and Exec keeps the packaged entry's file argument (`%f`: since M15.24 a lone
# file opens in the photo viewer, as `--view FILE`).
#
# On Wayland the dock finds a window's icon through its app id (org.dalica.Dalica) and an
# installed .desktop file of that name. An AppImage installs nothing, so without help every run
# would show a generic icon. So, best effort, on the first run and whenever the AppImage moves,
# this installs the .desktop entry (Exec pointing at the AppImage) and the hicolor icons into
# $XDG_DATA_HOME, then starts Dalica. A failure here never stops the launch.
#
# Opt out with DALICA_NO_DESKTOP_INTEGRATION=1. An external AppImage integrator (AppImageLauncher,
# appimaged, Gear Lever) that already manages this AppImage gets no second entry.
# Tested by packaging/linux/apprun-test.sh.

set -u

APPDIR="$(dirname "$(readlink -f "$0")")"
DESK_ID=org.dalica.Dalica

integrate() {
  case "${DALICA_NO_DESKTOP_INTEGRATION:-}" in
    1 | true | yes) return 0 ;;
  esac
  # The AppImage runtime exports $APPIMAGE; without it (an extracted AppDir) there is no path to
  # register.
  appimage="${APPIMAGE:-}"
  if [ -z "$appimage" ] || [ ! -f "$appimage" ]; then return 0; fi
  appimage="$(readlink -f "$appimage")" || return 0
  # Exec= is parsed by the desktop: a path with a quote, backquote, dollar, backslash, percent or
  # a line break would need escaping (or could add lines), so such a path is not integrated.
  case "$appimage" in
    *[\"\`\$\\%]* | *'
'*) return 0 ;;
  esac
  home="${HOME:-}"
  [ -n "$home" ] || return 0
  data="${XDG_DATA_HOME:-$home/.local/share}"
  src_desktop="$APPDIR/usr/share/applications/$DESK_ID.desktop"
  [ -f "$src_desktop" ] || return 0

  apps="$data/applications"
  mkdir -p "$apps" 2>/dev/null || return 0
  external=0
  if [ -n "${DESKTOPINTEGRATION:-}" ]; then
    external=1
  else
    for f in "$apps"/*.desktop; do
      if [ ! -f "$f" ] || [ "$f" = "$apps/$DESK_ID.desktop" ]; then continue; fi
      if grep '^Exec=' "$f" 2>/dev/null | grep -Fq "$appimage"; then
        external=1
        break
      fi
    done
  fi
  if [ "$external" = 1 ]; then
    # Another integrator owns the entry: remove ours if an earlier run left one; keep the icons.
    if [ -f "$apps/$DESK_ID.desktop" ]; then
      rm -f "$apps/$DESK_ID.desktop" 2>/dev/null || true
      if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$apps" >/dev/null 2>&1 || true
      fi
    fi
  else
    # Exec points at this AppImage (dalica is not on the host's PATH), its arguments (`%f`) kept;
    # TryExec, which would hide the entry, goes; everything else stays as packaged.
    tmp="$apps/.$DESK_ID.tmp.$$"
    esc="$(printf '%s' "$appimage" | sed 's/[\\&|]/\\&/g')"
    if sed -e "s|^Exec=[^ ]*|Exec=\"$esc\"|" -e '/^TryExec=/d' "$src_desktop" 2>/dev/null >"$tmp"; then
      if cmp -s "$tmp" "$apps/$DESK_ID.desktop"; then
        rm -f "$tmp"
      elif mv -f "$tmp" "$apps/$DESK_ID.desktop" 2>/dev/null; then
        if command -v update-desktop-database >/dev/null 2>&1; then
          update-desktop-database "$apps" >/dev/null 2>&1 || true
        fi
      else
        rm -f "$tmp"
      fi
    else
      rm -f "$tmp"
      return 0
    fi
  fi

  # The icons, file by file, so an unchanged install costs a few comparisons.
  src_icons="$APPDIR/usr/share/icons/hicolor"
  [ -d "$src_icons" ] || return 0
  icons="$data/icons/hicolor"
  (cd "$src_icons" && find . -type f) | while IFS= read -r rel; do
    mkdir -p "$icons/$(dirname "$rel")" 2>/dev/null || continue
    if ! cmp -s "$src_icons/$rel" "$icons/$rel"; then
      cp "$src_icons/$rel" "$icons/$rel" 2>/dev/null || true
    fi
  done
}

integrate
exec "$APPDIR/usr/bin/dalica" "$@"
