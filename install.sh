#!/bin/bash

# Build and install the ELD voxel screensaver, and make Omarchy use it.
# Usage: ./install.sh [--no-hook]   (--no-hook installs the binaries only)

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

BIN=$HOME/.local/bin
DATA=${XDG_DATA_HOME:-$HOME/.local/share}/eld-voxels
PROFILE=$HOME/.bash_profile
MENU=$HOME/.config/omarchy/extensions/omarchy-menu.jsonc
BEGIN="# >>> eld-voxels screensaver >>>"
END="# <<< eld-voxels screensaver <<<"

# Fresh Omarchy installs ship without Rust. Use rustup's cargo if it is
# installed but not on PATH yet, otherwise install Arch's rust package.
if ! command -v cargo >/dev/null && [[ -x $HOME/.cargo/bin/cargo ]]; then
  PATH=$HOME/.cargo/bin:$PATH
fi
if ! command -v cargo >/dev/null; then
  echo "cargo not found: installing Rust with pacman (needs sudo)"
  if command -v omarchy-pkg-add >/dev/null; then
    omarchy-pkg-add rust
  else
    sudo pacman -S --needed rust
  fi
fi

cargo build --release

install -Dm755 target/release/eld-voxels "$BIN/eld-voxels"
install -Dm755 bin/eld-screensaver "$BIN/eld-screensaver"
install -Dm755 bin/eld-screensaver-launch "$BIN/eld-screensaver-launch"
install -Dm644 config/foot.ini "$DATA/foot.ini"
echo "Installed eld-voxels, eld-screensaver and eld-screensaver-launch to $BIN"

[[ ${1:-} == "--no-hook" ]] && exit 0

# Omarchy's idle service starts the screensaver with
# `bash -lc omarchy-launch-screensaver`. A login-shell function of the same
# name takes precedence over the packaged command, so route it to ours.
touch "$PROFILE"
sed -i "/^$BEGIN\$/,/^$END\$/d" "$PROFILE"
cat >>"$PROFILE" <<HOOK
$BEGIN
# Omarchy's idle service runs \`bash -lc omarchy-launch-screensaver\`: use the ELD voxel screensaver.
omarchy-launch-screensaver() {
  if [[ -x "$BIN/eld-screensaver-launch" ]]; then
    "$BIN/eld-screensaver-launch" "\$@"
  else
    command omarchy-launch-screensaver "\$@"
  fi
}
$END
HOOK
echo "Hooked Omarchy's idle screensaver in $PROFILE"

# Point the menu's System > Screensaver entry at it too.
if [[ -f $MENU ]] && ! grep -q "eld-screensaver-launch" "$MENU"; then
  entry="  \"system.screensaver\": {\"icon\":\"󱄄\",\"label\":\"Screensaver\",\"action\":\"$BIN/eld-screensaver-launch force\"},"
  # Insert before the final closing brace.
  last=$(grep -n '^}' "$MENU" | tail -1 | cut -d: -f1)
  sed -i "${last}i\\  // eld-voxels: ELD voxel screensaver\\n$entry" "$MENU"
  echo "Updated System > Screensaver in $MENU"
fi

echo
echo "Preview it now:  eld-voxels            (n = next act, p = palette, any other key quits)"
echo "Full screen:     eld-screensaver-launch force"
