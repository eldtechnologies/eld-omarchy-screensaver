#!/bin/bash

# Remove the ELD voxel screensaver and restore Omarchy's stock screensaver.

set -euo pipefail

BIN=$HOME/.local/bin
DATA=${XDG_DATA_HOME:-$HOME/.local/share}/eld-voxels
PROFILE=$HOME/.bash_profile
MENU=$HOME/.config/omarchy/extensions/omarchy-menu.jsonc

rm -f "$BIN/eld-voxels" "$BIN/eld-screensaver" "$BIN/eld-screensaver-launch"
rm -rf "$DATA"

if [[ -f $PROFILE ]]; then
  sed -i '/^# >>> eld-voxels screensaver >>>$/,/^# <<< eld-voxels screensaver <<<$/d' "$PROFILE"
fi
if [[ -f $MENU ]]; then
  sed -i '/eld-voxels: ELD voxel screensaver/d; /eld-screensaver-launch/d' "$MENU"
fi

echo "Removed the ELD voxel screensaver; Omarchy's stock screensaver is back."
