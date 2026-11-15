#!/bin/bash
# Explicit uninstall only. Updaters must never invoke this script.
set -euo pipefail
resources="$(cd -- "$(dirname -- "$0")" && pwd -P)"
bundle="$(cd -- "$resources/../.." && pwd -P)"
if [[ "$bundle" != *.app ]] || [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Info.plist")" != "com.alkinum.aipass" ]]; then
  echo "This uninstaller must run from the installed AIPass application." >&2
  exit 1
fi
if /usr/sbin/lsof -t "$bundle/Contents/MacOS/aipass-desktop" >/dev/null 2>&1; then
  echo "Quit AIPass from its tray menu, then run this uninstaller again." >&2
  exit 1
fi
read -r -p 'Restore Agent configurations and move AIPass to Trash? [y/N] ' answer
case "$answer" in y|Y) ;; *) exit 1 ;; esac
"$resources/aipass-agent" --prepare-uninstall
mkdir -p -- "$HOME/.Trash"
destination="$(mktemp -d "$HOME/.Trash/AIPass-uninstall.XXXXXX")"
/bin/mv -- "$bundle" "$destination/"
echo "Agent configurations restored. AIPass moved to Trash. Your vault was retained."
