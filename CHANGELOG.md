# Changelog

All notable changes to this screensaver. The version is set in `Cargo.toml`;
`eld-voxels --version` reads it from there.

## 1.0.1 — 2026-10-23

- `install.sh` installs Rust when `cargo` is missing, as on a fresh Omarchy
  install, instead of failing with "cargo: command not found".

## 1.0.0 — 2026-10-11

First release.

- The ELD logo as about 4,800 voxels, rendered in 3D in the terminal with
  truecolor half-block cells, at 30 fps by default.
- Each cycle the logo bounces in from the sky, then plays four random acts
  (wave, explode, globe, torus, vortex, galaxy, helix) and lifts off again.
- Four palettes (Brand, Spectrum, Duo, Aurora), all built from the active
  Omarchy theme. Switching themes while it runs fades to the new colors.
- Synthwave grid floor with reflections and soft shadows, stars, shooting
  stars, dust, bloom and a vignette.
- Replaces Omarchy's idle screensaver and the menu's Screensaver entry;
  one window per monitor in foot, alacritty, ghostty or kitty. Exits on
  any key, mouse movement or focus loss.
- `install.sh` and `uninstall.sh`; `tools/png2mask.py` to use another logo.
- No dependencies beyond the Rust standard library.
