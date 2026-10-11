//! Reads the active Omarchy theme so the voxels match the desktop.

use std::collections::HashMap;
use std::path::PathBuf;

pub type Rgb = [f32; 3];

#[derive(Clone, Debug)]
pub struct Theme {
    pub accent: Rgb,
    pub background: Rgb,
    pub foreground: Rgb,
    pub red: Rgb,
    pub orange: Rgb,
    pub yellow: Rgb,
    pub green: Rgb,
    pub cyan: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
}

impl Default for Theme {
    // Tokyo Night, Omarchy's default theme.
    fn default() -> Theme {
        Theme {
            accent: hex("#7aa2f7").unwrap(),
            background: hex("#1a1b26").unwrap(),
            foreground: hex("#a9b1d6").unwrap(),
            red: hex("#f7768e").unwrap(),
            orange: hex("#ff9e64").unwrap(),
            yellow: hex("#e0af68").unwrap(),
            green: hex("#9ece6a").unwrap(),
            cyan: hex("#7dcfff").unwrap(),
            blue: hex("#7aa2f7").unwrap(),
            magenta: hex("#bb9af7").unwrap(),
        }
    }
}

fn hex(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_start_matches(['"', '\'']).trim_start_matches('#');
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() < 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok().map(|v| v as f32 / 255.0);
    Some([p(0)?, p(2)?, p(4)?])
}

fn theme_paths() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("ELD_VOXELS_THEME") {
        return vec![PathBuf::from(path)];
    }
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    vec![
        home.join(".local/state/omarchy/current/theme/colors.toml"),
        home.join(".config/omarchy/current/theme/colors.toml"),
    ]
}

impl Theme {
    pub fn load() -> Theme {
        Theme::read_source().map(|t| Theme::parse(&t)).unwrap_or_default()
    }

    /// The raw colors.toml of the active Omarchy theme, if there is one.
    pub fn read_source() -> Option<String> {
        theme_paths().iter().find_map(|p| std::fs::read_to_string(p).ok())
    }

    /// Move every color a fraction `k` of the way toward `target`.
    pub fn blend_toward(&mut self, target: &Theme, k: f32) {
        for (c, t) in [
            (&mut self.accent, target.accent),
            (&mut self.background, target.background),
            (&mut self.foreground, target.foreground),
            (&mut self.red, target.red),
            (&mut self.orange, target.orange),
            (&mut self.yellow, target.yellow),
            (&mut self.green, target.green),
            (&mut self.cyan, target.cyan),
            (&mut self.blue, target.blue),
            (&mut self.magenta, target.magenta),
        ] {
            *c = lerp(*c, t, k);
        }
    }

    pub fn parse(text: &str) -> Theme {
        let mut map = HashMap::new();
        for line in text.lines() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if let Some(c) = hex(v) {
                    map.insert(k.trim().to_string(), c);
                }
            }
        }
        let d = Theme::default();
        // Newer themes use names, older ones use the 16 terminal colors.
        let get = |names: &[&str], fallback: Rgb| {
            names.iter().find_map(|n| map.get(*n).copied()).unwrap_or(fallback)
        };
        let blue = get(&["blue", "color4"], d.blue);
        Theme {
            accent: get(&["accent", "blue", "color4"], blue),
            background: get(&["background"], d.background),
            foreground: get(&["foreground", "color7"], d.foreground),
            red: get(&["red", "color1"], d.red),
            orange: get(&["orange", "bright_red", "color9"], d.orange),
            yellow: get(&["yellow", "color3"], d.yellow),
            green: get(&["green", "color2"], d.green),
            cyan: get(&["cyan", "color6"], d.cyan),
            blue,
            magenta: get(&["magenta", "color5"], d.magenta),
        }
    }
}

pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub fn scale(a: Rgb, s: f32) -> Rgb {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// Sample a multi-stop gradient at t in [0, 1].
pub fn gradient(stops: &[Rgb], t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (t.floor() as usize).min(stops.len() - 2);
    lerp(stops[i], stops[i + 1], t - i as f32)
}

pub const WHITE: Rgb = [1.0, 1.0, 1.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Palette {
    /// Light, accent-tinted logo faces with an accent extrusion — the brand look.
    Brand,
    /// Full theme rainbow sweeping across the logo.
    Spectrum,
    /// Accent fading into magenta on a diagonal.
    Duo,
    /// Vertical green → cyan → magenta, like northern lights.
    Aurora,
}

pub const PALETTES: [Palette; 4] = [Palette::Brand, Palette::Spectrum, Palette::Duo, Palette::Aurora];

impl Theme {
    /// Color of a voxel at normalized logo coords (gx, gy in [0,1], gy=0 at top)
    /// and depth fraction (0 = front face layer).
    pub fn voxel_color(&self, p: Palette, gx: f32, gy: f32, depth: f32) -> Rgb {
        match p {
            Palette::Brand => {
                let face = lerp(lerp(WHITE, self.foreground, 0.15), self.accent, 0.3);
                lerp(face, self.accent, (depth * 0.9 + gy * 0.15).min(1.0))
            }
            Palette::Spectrum => {
                let stops = [self.red, self.orange, self.yellow, self.green, self.cyan, self.blue, self.magenta];
                lerp(gradient(&stops, gx), WHITE, 0.12 * (1.0 - depth))
            }
            Palette::Duo => {
                let c = gradient(&[self.accent, self.magenta, self.cyan], gx * 0.75 + (1.0 - gy) * 0.25);
                lerp(c, scale(c, 0.6), depth)
            }
            Palette::Aurora => {
                let c = gradient(&[self.magenta, self.cyan, self.green], gy * 0.8 + gx * 0.2);
                lerp(lerp(c, WHITE, 0.1), scale(c, 0.65), depth)
            }
        }
    }
}
