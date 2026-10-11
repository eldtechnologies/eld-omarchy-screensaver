//! The voxel logo and its choreography.
//!
//! Every act starts and ends with the logo assembled at its home position, so
//! acts can be shuffled freely. Within an act, phases either morph voxels from
//! a snapshot of where they are toward a (possibly moving) target shape, hold a
//! shape, or run a small physics simulation.

use std::collections::VecDeque;

use crate::math::*;
use crate::theme::{Palette, Rgb, Theme, PALETTES};

pub const EMBEDDED_MASK: &str = include_str!("../assets/eld.mask");

/// Face order shared with the renderer: +x, -x, +y, -y, +z, -z.
pub const FACE_DIRS: [(i32, i32, i32); 6] = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)];

pub struct Voxel {
    pub home: V3,
    /// Normalized logo coordinates, gy = 0 at the top row.
    pub gx: f32,
    pub gy: f32,
    /// 0 = front layer.
    pub layer: usize,
    pub seed: [f32; 4],
    pub neighbors: [i32; 6],

    pub pos: V3,
    pub rot: V3,
    pub scale: f32,
    pub glow: f32,
    pub color: Rgb,
    target_color: Rgb,
    pub at_home: bool,

    vel: V3,
    spin: V3,
    from_pos: V3,
    from_rot: V3,
    from_scale: f32,
    from_glow: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Home,
    Sky,
    Shine,
    Wave,
    Globe,
    Torus,
    Vortex,
    Galaxy,
    Helix,
}

#[derive(Clone, Copy, Debug)]
pub enum Stagger {
    None,
    LeftToRight,
    BottomUp,
    Random,
    CenterOut,
}

#[derive(Clone, Copy, Debug)]
pub enum Phase {
    Set(Shape),
    Morph { to: Shape, dur: f32, ease: Ease, stagger: Stagger, arc: f32 },
    Hold { shape: Shape, dur: f32 },
    Explode { dur: f32 },
    Pause { dur: f32 },
}

impl Phase {
    fn dur(&self) -> f32 {
        match *self {
            Phase::Set(_) => 0.0,
            Phase::Morph { dur, .. } | Phase::Hold { dur, .. } | Phase::Explode { dur } | Phase::Pause { dur } => dur,
        }
    }
}

/// Acts that can be shuffled into the middle of each cycle.
pub const ACTS: [&str; 7] = ["wave", "explode", "globe", "torus", "vortex", "galaxy", "helix"];

struct State {
    pos: V3,
    rot: V3,
    scale: f32,
    glow: f32,
}

pub struct Scene {
    pub voxels: Vec<Voxel>,
    pub width: usize,
    pub height: usize,
    pub depth: usize,
    /// Center of the assembled logo.
    pub center: V3,
    pub time: f32,
    /// Colors currently on screen; they fade toward `theme_target`.
    pub theme: Theme,
    theme_target: Theme,
    theme_source: Option<String>,
    theme_poll: f32,
    pub act_name: &'static str,
    /// Camera shake/punch, decays to 0.
    pub punch: f32,

    queue: VecDeque<(Phase, &'static str)>,
    phase: Phase,
    phase_t: f32,
    palette_idx: usize,
    rng: Rng,
    forced_act: Option<&'static str>,
    last_acts: Vec<&'static str>,
    cycles: usize,
}

const BASE_Y: f32 = 3.0;

impl Scene {
    pub fn new(mask: &str, depth: usize, seed: u64, forced_act: Option<&'static str>) -> Scene {
        let rows: Vec<&str> = mask.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
        let height = rows.len();
        let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(1);
        let filled = |x: i32, y: i32| -> bool {
            x >= 0
                && y >= 0
                && (y as usize) < height
                && rows[y as usize].chars().nth(x as usize).is_some_and(|c| c != '.' && c != ' ')
        };
        let mut rng = Rng::new(seed);
        let mut voxels = Vec::new();
        let mut index = vec![-1i32; width * height * depth];
        let front_z = (depth as f32 - 1.0) / 2.0;
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                if !filled(x, y) {
                    continue;
                }
                for layer in 0..depth {
                    let home = v3(
                        x as f32 - width as f32 / 2.0 + 0.5,
                        BASE_Y + (height as i32 - 1 - y) as f32 + 0.5,
                        front_z - layer as f32,
                    );
                    index[(layer * height + y as usize) * width + x as usize] = voxels.len() as i32;
                    voxels.push(Voxel {
                        home,
                        gx: (x as f32 + 0.5) / width as f32,
                        gy: (y as f32 + 0.5) / height as f32,
                        layer,
                        seed: [rng.f32(), rng.f32(), rng.f32(), rng.f32()],
                        neighbors: [-1; 6],
                        pos: home,
                        rot: V3::default(),
                        scale: 1.0,
                        glow: 0.0,
                        color: [1.0; 3],
                        target_color: [1.0; 3],
                        at_home: true,
                        vel: V3::default(),
                        spin: V3::default(),
                        from_pos: home,
                        from_rot: V3::default(),
                        from_scale: 1.0,
                        from_glow: 0.0,
                    });
                }
            }
        }
        // Grid neighbors, in world axes (grid y is flipped, layer grows toward -z).
        for v in voxels.iter_mut() {
            let gx = (v.gx * width as f32) as i32;
            let gy = (v.gy * height as f32) as i32;
            let l = v.layer as i32;
            for (f, &(dx, dy, dz)) in FACE_DIRS.iter().enumerate() {
                let (nx, ny, nl) = (gx + dx, gy - dy, l - dz);
                if nx >= 0 && ny >= 0 && nl >= 0 && (nx as usize) < width && (ny as usize) < height && (nl as usize) < depth {
                    v.neighbors[f] = index[(nl as usize * height + ny as usize) * width + nx as usize];
                }
            }
        }

        let mut scene = Scene {
            voxels,
            width,
            height,
            depth,
            center: v3(0.0, BASE_Y + height as f32 / 2.0, 0.0),
            time: 0.0,
            theme: Theme::load(),
            theme_target: Theme::load(),
            theme_source: Theme::read_source(),
            theme_poll: 0.0,
            act_name: "intro",
            punch: 0.0,
            queue: VecDeque::new(),
            phase: Phase::Pause { dur: 0.0 },
            phase_t: 0.0,
            palette_idx: 0,
            rng,
            forced_act,
            last_acts: Vec::new(),
            cycles: 0,
        };
        scene.apply_palette();
        scene.snap_colors();
        scene.set_shape(Shape::Sky);
        scene
    }

    /// Set each voxel's target color; `update` fades the voxels toward it.
    fn apply_palette(&mut self) {
        let p: Palette = PALETTES[self.palette_idx % PALETTES.len()];
        let d = (self.depth.max(2) - 1) as f32;
        for v in self.voxels.iter_mut() {
            v.target_color = self.theme_target.voxel_color(p, v.gx, v.gy, v.layer as f32 / d);
        }
    }

    fn snap_colors(&mut self) {
        self.theme = self.theme_target.clone();
        for v in self.voxels.iter_mut() {
            v.color = v.target_color;
        }
    }

    /// Follow Omarchy theme switches live: re-read colors.toml about once a second.
    fn watch_theme(&mut self, dt: f32) {
        self.theme_poll += dt;
        if self.theme_poll < 1.0 {
            return;
        }
        self.theme_poll = 0.0;
        let source = Theme::read_source();
        if source != self.theme_source {
            self.theme_target = source.as_deref().map(Theme::parse).unwrap_or_default();
            self.theme_source = source;
            self.apply_palette();
        }
    }

    pub fn next_palette(&mut self) {
        self.palette_idx += 1;
        self.apply_palette();
    }

    /// Jump straight to the next act (keyboard shortcut in preview mode).
    pub fn skip_act(&mut self) {
        while self.queue.front().is_some_and(|(_, name)| *name == self.act_name) {
            self.queue.pop_front();
        }
        self.phase = Phase::Morph { to: Shape::Home, dur: 1.2, ease: Ease::InOut, stagger: Stagger::None, arc: 0.0 };
        self.phase_t = 0.0;
        self.snapshot();
    }

    fn build_cycle(&mut self) {
        // The voxels are up in the sky now, so switch palettes without a fade.
        if self.cycles > 0 {
            self.palette_idx += 1;
        }
        self.apply_palette();
        self.snap_colors();
        self.cycles += 1;

        let q = &mut self.queue;
        q.push_back((Phase::Set(Shape::Sky), "intro"));
        q.push_back((Phase::Morph { to: Shape::Home, dur: 4.5, ease: Ease::OutBounce, stagger: Stagger::LeftToRight, arc: 0.0 }, "intro"));
        q.push_back((Phase::Hold { shape: Shape::Shine, dur: 4.0 }, "intro"));

        let mut pool: Vec<&'static str> = ACTS.iter().copied().filter(|a| !self.last_acts.contains(a)).collect();
        let mut picks = Vec::new();
        if let Some(a) = self.forced_act.take() {
            picks.push(a);
            pool.retain(|p| *p != a);
        }
        while picks.len() < 4 && !pool.is_empty() {
            picks.push(pool.remove(self.rng.below(pool.len())));
        }
        self.last_acts = picks.clone();
        for name in picks {
            for phase in act_phases(name) {
                self.queue.push_back((phase, name));
            }
            self.queue.push_back((Phase::Hold { shape: Shape::Shine, dur: 3.0 }, name));
        }

        self.queue.push_back((Phase::Morph { to: Shape::Sky, dur: 3.0, ease: Ease::InCubic, stagger: Stagger::Random, arc: 0.0 }, "outro"));
        self.queue.push_back((Phase::Pause { dur: 1.0 }, "outro"));
    }

    fn snapshot(&mut self) {
        for v in self.voxels.iter_mut() {
            v.rot = v3(wrap_angle(v.rot.x), wrap_angle(v.rot.y), wrap_angle(v.rot.z));
            v.from_pos = v.pos;
            v.from_rot = v.rot;
            v.from_scale = v.scale;
            v.from_glow = v.glow;
        }
    }

    fn set_shape(&mut self, shape: Shape) {
        let (t, n) = (self.time, self.voxels.len());
        for i in 0..n {
            let s = self.shape_state(shape, &self.voxels[i], i, t, 0.0, 1.0);
            let v = &mut self.voxels[i];
            v.pos = s.pos;
            v.rot = s.rot;
            v.scale = s.scale;
            v.glow = s.glow;
        }
    }

    fn start_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_t = 0.0;
        self.snapshot();
        match phase {
            Phase::Set(shape) => self.set_shape(shape),
            Phase::Explode { .. } => {
                let origin = self.center - v3(0.0, self.height as f32 * 0.6, 0.0);
                for v in self.voxels.iter_mut() {
                    let jitter = v3(v.seed[0] - 0.5, v.seed[1] - 0.5, v.seed[2] - 0.5);
                    let dir = ((v.home - origin).norm() + jitter * 0.8).norm();
                    v.vel = dir * (7.0 + v.seed[3] * 15.0) + v3(0.0, 12.0 + v.seed[1] * 12.0, 4.0 * (v.seed[2] - 0.3));
                    v.spin = v3(v.seed[0] - 0.5, v.seed[1] - 0.5, v.seed[3] - 0.5) * 16.0;
                }
                self.punch = 1.0;
            }
            _ => {}
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        self.phase_t += dt;
        self.punch = (self.punch - dt * 1.6).max(0.0);

        self.watch_theme(dt);
        let fade = 1.0 - (-dt * 2.5).exp();
        self.theme.blend_toward(&self.theme_target, fade);
        for v in self.voxels.iter_mut() {
            v.color = crate::theme::lerp(v.color, v.target_color, fade);
        }

        while self.phase_t >= self.phase.dur() {
            if self.queue.is_empty() {
                self.build_cycle();
            }
            let carry = (self.phase_t - self.phase.dur()).max(0.0);
            let (phase, name) = self.queue.pop_front().unwrap();
            self.act_name = name;
            self.start_phase(phase);
            self.phase_t = carry;
        }

        let (t, pt) = (self.time, self.phase_t);
        let n = self.voxels.len();
        match self.phase {
            Phase::Set(_) | Phase::Pause { .. } => {}
            Phase::Hold { shape, dur } => {
                for i in 0..n {
                    let s = self.shape_state(shape, &self.voxels[i], i, t, pt, dur);
                    let v = &mut self.voxels[i];
                    v.pos = s.pos;
                    v.rot = s.rot;
                    v.scale = s.scale;
                    v.glow = s.glow;
                }
            }
            Phase::Morph { to, dur, ease, stagger, arc } => {
                let s = clamp01(pt / dur);
                let spread = match stagger {
                    Stagger::None => 0.0,
                    _ => 0.5,
                };
                let rot_ease = match ease {
                    Ease::OutBounce => Ease::OutCubic,
                    e => e,
                };
                for i in 0..n {
                    let target = self.shape_state(to, &self.voxels[i], i, t, pt, dur);
                    let v = &mut self.voxels[i];
                    let delay = match stagger {
                        Stagger::None => 0.0,
                        Stagger::LeftToRight => v.gx * 0.9 + v.seed[0] * 0.1,
                        Stagger::BottomUp => (1.0 - v.gy) * 0.9 + v.seed[0] * 0.1,
                        Stagger::Random => v.seed[0],
                        Stagger::CenterOut => ((v.gx - 0.5).abs() * 1.4 + (v.gy - 0.5).abs() * 0.6).min(1.0),
                    };
                    let p = clamp01((s - delay * spread) / (1.0 - spread));
                    let e = ease.apply(p);
                    let lift = (PI * p).sin() * arc * (0.6 + 0.4 * v.seed[1]);
                    v.pos = v.from_pos.lerp(target.pos, e) + v3(0.0, lift, 0.0);
                    v.rot = v.from_rot.lerp(target.rot, rot_ease.apply(p));
                    v.scale = mix(v.from_scale, target.scale, rot_ease.apply(p));
                    v.glow = mix(v.from_glow, target.glow, p) + (PI * p).sin() * 0.45;
                }
            }
            Phase::Explode { .. } => {
                let floor = 0.5;
                for v in self.voxels.iter_mut() {
                    v.vel.y -= 32.0 * dt;
                    v.pos += v.vel * dt;
                    v.rot += v.spin * dt;
                    if v.pos.y < floor {
                        v.pos.y = floor;
                        v.vel.y = -v.vel.y * 0.42;
                        v.vel.x *= 0.7;
                        v.vel.z *= 0.7;
                        v.spin = v.spin * 0.6;
                    }
                    v.glow = (1.4 - pt * 1.8).max(0.0) + (v.vel.len() * 0.012).min(0.3);
                }
            }
        }

        for i in 0..n {
            let v = &self.voxels[i];
            let home = (v.pos - v.home).dot(v.pos - v.home) < 1e-4 && v.rot.dot(v.rot) < 1e-6 && (v.scale - 1.0).abs() < 1e-3;
            self.voxels[i].at_home = home;
        }
    }

    fn shape_state(&self, shape: Shape, v: &Voxel, i: usize, t: f32, pt: f32, pdur: f32) -> State {
        let home = State { pos: v.home, rot: V3::default(), scale: 1.0, glow: 0.0 };
        let (w, h) = (self.width as f32, self.height as f32);
        let depth_off = (self.depth as f32 - 1.0) / 2.0 - v.layer as f32;
        match shape {
            Shape::Home => home,
            Shape::Sky => State {
                pos: v.home + v3((v.seed[2] - 0.5) * 6.0, 60.0 + v.seed[1] * 40.0, (v.seed[3] - 0.5) * 6.0),
                rot: v3(v.seed[0] - 0.5, v.seed[2] - 0.5, v.seed[3] - 0.5) * 6.0,
                scale: 1.0,
                glow: 0.6,
            },
            Shape::Shine => {
                // A diagonal band of light sweeping across the logo.
                let period = 2.8;
                let sweep = (pt % period) / period * 1.8 - 0.4;
                let d = v.gx * 0.8 + (1.0 - v.gy) * 0.2 - sweep;
                State { glow: (-d * d * 90.0).exp() * 1.3, ..home }
            }
            Shape::Wave => {
                let env = smoothstep(0.0, 1.5, pt) * smoothstep(pdur, pdur - 1.5, pt);
                let ph = v.gx * 11.0 - t * 3.2 + v.gy * 1.5;
                let z = ph.sin() * 3.2 * env;
                let y = (ph * 0.5 + 1.0).sin() * 1.2 * env;
                State {
                    pos: v.home + v3(0.0, y, z),
                    rot: v3(-ph.cos() * 0.5 * env, 0.0, 0.0),
                    scale: 1.0,
                    glow: ph.sin().max(0.0).powi(6) * 0.9 * env,
                }
            }
            Shape::Globe => {
                let r = w / TAU + depth_off;
                let lon = (v.gx - 0.5) * TAU + t * 0.55;
                let lat = (0.5 - v.gy) * h / (w / TAU);
                let local = v3(r * lat.cos() * lon.sin(), r * lat.sin(), r * lat.cos() * lon.cos());
                let tilt = M3::rz(0.25);
                let orient = tilt * M3::ry(lon) * M3::rx(-lat);
                State {
                    pos: self.center + v3(0.0, 2.0, 0.0) + tilt.apply(local),
                    rot: orient.to_euler(),
                    scale: 1.0,
                    glow: 0.1,
                }
            }
            Shape::Torus => {
                let big_r = w / TAU;
                let r = h / TAU + depth_off * 0.8;
                let u = (v.gx - 0.5) * TAU + t * 0.45;
                let a = (0.5 - v.gy) * TAU;
                let local = v3((big_r + r * a.cos()) * u.sin(), r * a.sin(), (big_r + r * a.cos()) * u.cos());
                let tilt = M3::rx(0.55 + (t * 0.3).sin() * 0.2) * M3::rz((t * 0.21).sin() * 0.25);
                let orient = tilt * M3::ry(u) * M3::rx(-a);
                State {
                    pos: self.center + v3(0.0, 3.0, 0.0) + tilt.apply(local),
                    rot: orient.to_euler(),
                    scale: 1.0,
                    glow: 0.1,
                }
            }
            Shape::Vortex => {
                // A funnel whose spine sways, with faster spin near the bottom.
                let k = v.seed[0];
                let y = 1.0 + k * 38.0;
                let r = 1.5 + k.powf(1.4) * 17.0 + v.seed[1] * (1.0 + k * 5.0);
                let a = v.seed[2] * TAU + t * (3.6 - k * 2.2);
                let spine = v3((y * 0.11 - t * 1.4).sin() * 4.0 * k, 0.0, (y * 0.09 - t * 1.1).cos() * 2.5 * k);
                State {
                    pos: spine + v3(r * a.cos(), y + (t * 2.0 + v.seed[3] * TAU).sin() * 0.8, r * a.sin()),
                    rot: v3(t * (v.seed[1] - 0.5) * 4.0, -a, t * (v.seed[3] - 0.5) * 4.0),
                    scale: 0.75,
                    glow: 0.15 + k * 0.35,
                }
            }
            Shape::Galaxy => {
                let arm = (i % 3) as f32;
                let d = 1.5 + v.seed[0].powf(0.8) * 28.0;
                let a = arm * TAU / 3.0 + d * 0.22 + (v.seed[1] - 0.5) * 0.7 - t * (1.1 / (1.0 + d * 0.06));
                let thick = (v.seed[2] - 0.5) * 3.0 * (1.0 - d / 32.0);
                let tilt = M3::rx(0.75) * M3::rz(0.15);
                let local = v3(d * a.cos(), thick, d * a.sin());
                State {
                    pos: self.center + v3(0.0, 4.0, 0.0) + tilt.apply(local),
                    rot: (tilt * M3::ry(-a)).to_euler(),
                    scale: 0.85,
                    glow: (1.0 - d / 12.0).max(0.0) * 1.2 + 0.08,
                }
            }
            Shape::Helix => {
                let k = v.seed[0];
                let y = 1.5 + k * 36.0;
                let base = k * TAU * 2.2 + t * 1.3;
                let r = 8.0;
                let pos = if i % 6 == 0 {
                    // Rungs between the two strands.
                    let p1 = v3(r * base.cos(), y, r * base.sin());
                    let p2 = v3(-r * base.cos(), y, -r * base.sin());
                    p1.lerp(p2, v.seed[1])
                } else {
                    let a = base + if i % 2 == 0 { 0.0 } else { PI };
                    let rr = r + (v.seed[1] - 0.5) * 2.5;
                    v3(rr * a.cos(), y + (v.seed[2] - 0.5) * 1.5, rr * a.sin())
                };
                State {
                    pos,
                    rot: v3(0.0, -base, 0.0),
                    scale: 0.9,
                    glow: if i % 6 == 0 { 0.5 } else { 0.12 },
                }
            }
        }
    }
}

fn act_phases(name: &str) -> Vec<Phase> {
    use Phase::*;
    let morph = |to, dur, ease, stagger| Morph { to, dur, ease, stagger, arc: 0.0 };
    match name {
        "wave" => vec![Hold { shape: Shape::Wave, dur: 10.0 }],
        "explode" => vec![
            Explode { dur: 3.8 },
            Morph { to: Shape::Home, dur: 3.4, ease: Ease::InOut, stagger: Stagger::Random, arc: 14.0 },
        ],
        "globe" => vec![
            morph(Shape::Globe, 3.2, Ease::InOut, Stagger::LeftToRight),
            Hold { shape: Shape::Globe, dur: 7.0 },
            morph(Shape::Home, 3.0, Ease::InOut, Stagger::LeftToRight),
        ],
        "torus" => vec![
            morph(Shape::Torus, 3.2, Ease::InOut, Stagger::CenterOut),
            Hold { shape: Shape::Torus, dur: 7.0 },
            morph(Shape::Home, 3.0, Ease::InOut, Stagger::CenterOut),
        ],
        "vortex" => vec![
            morph(Shape::Vortex, 3.5, Ease::InOut, Stagger::BottomUp),
            Hold { shape: Shape::Vortex, dur: 6.0 },
            Morph { to: Shape::Home, dur: 3.5, ease: Ease::InOut, stagger: Stagger::Random, arc: 4.0 },
        ],
        "galaxy" => vec![
            morph(Shape::Galaxy, 3.5, Ease::InOut, Stagger::CenterOut),
            Hold { shape: Shape::Galaxy, dur: 7.0 },
            morph(Shape::Home, 3.5, Ease::InOut, Stagger::Random),
        ],
        "helix" => vec![
            morph(Shape::Helix, 3.5, Ease::InOut, Stagger::BottomUp),
            Hold { shape: Shape::Helix, dur: 6.5 },
            morph(Shape::Home, 3.5, Ease::InOut, Stagger::BottomUp),
        ],
        _ => vec![Hold { shape: Shape::Shine, dur: 3.0 }],
    }
}
