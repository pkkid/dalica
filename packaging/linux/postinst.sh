#!/bin/sh
# The .deb's and the .rpm's post-install and post-remove script: refresh the desktop database and
# the icon cache, so the menu entry and its icon appear (and go) at once. Every tool is optional.
#
# Adapted from PhotoCraft packaging/linux/postinst.sh @ d98bee4c (MIT OR Apache-2.0), without the
# MIME database (Dalica installs no MIME types).
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
