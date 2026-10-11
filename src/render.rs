//! CPU software renderer: sky, reflective grid floor with soft shadows,
//! z-buffered voxel cubes with outlines, dust motes, meteors and bloom.

use crate::math::*;
use crate::scene::{Scene, FACE_DIRS};
use crate::theme::{lerp, scale, Rgb, WHITE};

pub struct Camera {
    pub pos: V3,
    right: V3,
    up: V3,
    fwd: V3,
    f: f32,
    cx: f32,
    cy: f32,
    /// Height / width of one output pixel.
    aspect: f32,
}

impl Camera {
    /// The camera drifts on a slow orbit in front of the logo.
    pub fn orbit(scene: &Scene, t: f32, w: usize, h: usize, aspect: f32) -> Camera {
        let target = scene.center + v3(0.0, 1.0, 0.0);
        let yaw = 0.5 * (t * 0.09).sin() + 0.22 * (t * 0.031 + 1.0).sin();
        let pitch = 0.17 + 0.07 * (t * 0.05).sin();
        let punch = scene.punch * scene.punch;
        let dist = 92.0 * (1.0 + 0.06 * (t * 0.07).sin()) - punch * 6.0;
        let shake = v3((t * 53.0).sin(), (t * 61.0).cos(), 0.0) * (punch * 0.6);
        let pos = target + v3(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * dist + shake;
        let fwd = (target - pos).norm();
        let right = fwd.cross(v3(0.0, 1.0, 0.0)).norm();
        let up = right.cross(fwd);

        // Fit the logo width and the tallest shapes into the frame.
        let logo_w = scene.width as f32 + 8.0;
        let f_w = 0.86 * w as f32 * 92.0 / logo_w;
        let f_h = 0.95 * h as f32 * aspect * 92.0 / 46.0;
        Camera { pos, right, up, fwd, f: f_w.min(f_h), cx: w as f32 / 2.0, cy: h as f32 * 0.54, aspect }
    }

    fn view(&self, p: V3) -> V3 {
        let d = p - self.pos;
        v3(d.dot(self.right), d.dot(self.up), d.dot(self.fwd))
    }

    fn project_view(&self, v: V3) -> (f32, f32) {
        (self.cx + v.x * self.f / v.z, self.cy - v.y * self.f / (v.z * self.aspect))
    }

}

#[derive(Clone, Copy)]
struct SV {
    x: f32,
    y: f32,
    iz: f32,
    u: f32,
    v: f32,
}

struct Star {
    dir: V3,
    bright: f32,
    phase: f32,
    speed: f32,
    tint: f32,
}

struct Mote {
    base: V3,
    phase: f32,
}

const SHADOW_RES: f32 = 2.0; // cells per world unit
const SHADOW_W: usize = 300;
const SHADOW_H: usize = 220;

pub struct Renderer {
    pub w: usize,
    pub h: usize,
    pub aspect: f32,
    color: Vec<Rgb>,
    emit: Vec<Rgb>,
    floor_t: Vec<f32>,
    // Scratch voxel buffer, reused for the reflection and main passes.
    g_col: Vec<Rgb>,
    g_glow: Vec<f32>,
    g_iz: Vec<f32>,
    shadow: Vec<f32>,
    shadow_tmp: Vec<f32>,
    small: Vec<Rgb>,
    small_tmp: Vec<Rgb>,
    pub out: Vec<u8>,
    stars: Vec<Star>,
    motes: Vec<Mote>,
    light: V3,
    /// Seconds spent per stage: shadow, sky+floor, reflection, voxels, bloom, tonemap.
    pub timings: [f64; 6],
    tone_lut: Vec<u8>,
    quads: Vec<Quad>,
}

impl Renderer {
    pub fn new(w: usize, h: usize, aspect: f32, seed: u64) -> Renderer {
        let n = w * h;
        let mut rng = Rng::new(seed ^ 0x5eed);
        let stars = (0..420)
            .map(|_| {
                let a = rng.range(0.0, TAU);
                let y = rng.range(0.02, 1.0).powf(1.6);
                let r = (1.0 - y * y).sqrt();
                Star {
                    dir: v3(r * a.sin(), y, -r * a.cos()),
                    bright: rng.range(0.15, 1.0).powi(3),
                    phase: rng.range(0.0, TAU),
                    speed: rng.range(0.5, 3.0),
                    tint: rng.f32(),
                }
            })
            .collect();
        let motes = (0..160)
            .map(|_| Mote { base: v3(rng.range(-70.0, 70.0), rng.range(0.0, 50.0), rng.range(-50.0, 40.0)), phase: rng.range(0.0, TAU) })
            .collect();
        let (sw, sh) = (w.div_ceil(4), h.div_ceil(4));
        Renderer {
            w,
            h,
            aspect,
            color: vec![[0.0; 3]; n],
            emit: vec![[0.0; 3]; n],
            floor_t: vec![0.0; n],
            g_col: vec![[0.0; 3]; n],
            g_glow: vec![0.0; n],
            g_iz: vec![0.0; n],
            shadow: vec![0.0; SHADOW_W * SHADOW_H],
            shadow_tmp: vec![0.0; SHADOW_W * SHADOW_H],
            small: vec![[0.0; 3]; sw * sh],
            small_tmp: vec![[0.0; 3]; sw * sh],
            out: vec![0; n * 3],
            stars,
            motes,
            light: v3(-0.45, 0.85, 0.55).norm(),
            timings: [0.0; 6],
            tone_lut: tone_lut(),
            quads: Vec::new(),
        }
    }

    pub fn render(&mut self, scene: &Scene, cam: &Camera) {
        let t = scene.time;
        let mut clock = std::time::Instant::now();
        let mut lap = |k: usize, timings: &mut [f64; 6]| {
            timings[k] += clock.elapsed().as_secs_f64();
            clock = std::time::Instant::now();
        };
        self.build_shadow(scene);
        lap(0, &mut self.timings);
        self.sky_and_floor(scene, cam);
        self.draw_stars(scene, cam, t);
        lap(1, &mut self.timings);

        // Reflection: mirror the voxels in the floor and blend them in.
        self.draw_voxels(scene, cam, true);
        for i in 0..self.w * self.h {
            if self.g_iz[i] > 0.0 && self.floor_t[i] > 0.0 {
                let a = 0.32 * (1.0 - (self.floor_t[i] / 260.0)).max(0.0);
                let c = self.g_col[i];
                let g = 1.0 + self.g_glow[i];
                self.color[i] = lerp(self.color[i], scale(c, g), a);
                self.emit[i] = lerp(self.emit[i], scale(c, self.g_glow[i] * 0.5), a);
            }
        }

        lap(2, &mut self.timings);
        // Main pass.
        self.draw_voxels(scene, cam, false);
        for i in 0..self.w * self.h {
            if self.g_iz[i] > 0.0 {
                let c = self.g_col[i];
                let g = self.g_glow[i];
                self.color[i] = lerp(c, WHITE, (g * 0.35).min(0.6));
                let e = 0.1 + g * 0.9;
                self.emit[i] = scale(c, e);
            }
        }

        lap(3, &mut self.timings);
        self.draw_motes(scene, cam, t);
        self.bloom();
        lap(4, &mut self.timings);
        self.tonemap();
        lap(5, &mut self.timings);
    }

    fn build_shadow(&mut self, scene: &Scene) {
        self.shadow.iter_mut().for_each(|s| *s = 0.0);
        let l = self.light;
        let (ox, oz) = (SHADOW_W as f32 / 2.0, SHADOW_H as f32 / 2.0);
        for v in &scene.voxels {
            let y = v.pos.y;
            if !(0.0..80.0).contains(&y) || v.scale < 0.05 {
                continue;
            }
            let x = (v.pos.x - l.x / l.y * y) * SHADOW_RES + ox;
            let z = (v.pos.z - l.z / l.y * y) * SHADOW_RES + oz;
            let (xi, zi) = (x as i32, z as i32);
            let k = v.scale * (1.0 - y / 80.0) * 0.5;
            for dz in 0..2 {
                for dx in 0..2 {
                    let (sx, sz) = (xi + dx, zi + dz);
                    if sx >= 0 && sz >= 0 && (sx as usize) < SHADOW_W && (sz as usize) < SHADOW_H {
                        self.shadow[sz as usize * SHADOW_W + sx as usize] += k;
                    }
                }
            }
        }
        box_blur_f32(&mut self.shadow, &mut self.shadow_tmp, SHADOW_W, SHADOW_H, 2);
        box_blur_f32(&mut self.shadow, &mut self.shadow_tmp, SHADOW_W, SHADOW_H, 2);
    }

    fn sky_and_floor(&mut self, scene: &Scene, cam: &Camera) {
        let th = &scene.theme;
        let accent = th.accent;
        let deep = scale(th.background, 0.35);
        let horizon = lerp(scale(th.background, 0.8), accent, 0.35);
        let floor_base = scale(th.background, 0.45);
        let grid = 4.0;
        let add = |a: Rgb, b: Rgb| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
        // The camera never rolls, so `right` is horizontal: the unnormalized ray's
        // y component (and with it the floor distance) is constant along a row,
        // and the floor hit point moves linearly across it.
        let ax = 1.0 / cam.f;
        let x0 = (0.5 - cam.cx) * ax;
        let w = self.w;
        let shadow_buf = &self.shadow;
        let rows_per = self.h.div_ceil(threads());
        let bands = self
            .color
            .chunks_mut(rows_per * w)
            .zip(self.emit.chunks_mut(rows_per * w))
            .zip(self.floor_t.chunks_mut(rows_per * w))
            .enumerate();
        std::thread::scope(|sc| {
            for (band, ((color, emit), floor_t)) in bands {
                sc.spawn(move || {
        for ly in 0..color.len() / w {
            let py = band * rows_per + ly;
            let b = -(py as f32 + 0.5 - cam.cy) * cam.aspect / cam.f;
            let row = cam.fwd + cam.up * b;
            let center = (row + cam.right * ((w as f32 / 2.0 - cam.cx) * ax)).norm();
            let glow_band = (-(center.y.abs()) * 28.0).exp();
            let band = scale(accent, glow_band * 0.25);
            let line = &mut color[ly * w..(ly + 1) * w];
            let emit = &mut emit[ly * w..(ly + 1) * w];
            let floor_t = &mut floor_t[ly * w..(ly + 1) * w];
            if row.y >= -1e-4 {
                let k = smoothstep(0.0, 0.55, center.y);
                let c = add(lerp(horizon, deep, k), scale(accent, glow_band * 0.3));
                line.fill(c);
                emit.fill(scale(accent, glow_band * 0.12));
                floor_t.fill(0.0);
                continue;
            }
            let tp = -cam.pos.y / row.y;
            // Distance and footprint use the row center; they vary slowly across a row.
            let t = tp * (row + cam.right * ((w as f32 / 2.0 - cam.cx) * ax)).len();
            let fw = t / cam.f;
            let fz = fw / (-center.y).max(0.05) * cam.aspect;
            let fade = (1.0 - fz / 1.6).max(0.0) * (-t * 0.004).exp();
            let fog = 1.0 - (-t * 0.0045).exp();
            let base = lerp(floor_base, horizon, fog * 0.9);
            let hz = cam.pos.z + (row.z + cam.right.z * x0) * tp;
            let hx0 = cam.pos.x + (row.x + cam.right.x * x0) * tp;
            let dhx = cam.right.x * ax * tp;
            let dhz = cam.right.z * ax * tp;
            let sz_row = hz * SHADOW_RES + SHADOW_H as f32 / 2.0;
            let sdz = dhz * SHADOW_RES;
            for px in 0..w {
                let fx = px as f32;
                let hx = hx0 + dhx * fx;
                let hzp = hz + dhz * fx;
                let l = (grid_line(hx, grid, fw).max(grid_line(hzp, grid, fz)) * fade).min(1.0);
                let sx = (hx * SHADOW_RES + SHADOW_W as f32 / 2.0) as i32;
                let sz = (sz_row + sdz * fx) as i32;
                let shadow = if sx >= 0 && sz >= 0 && (sx as usize) < SHADOW_W && (sz as usize) < SHADOW_H {
                    shadow_buf[sz as usize * SHADOW_W + sx as usize].min(1.0)
                } else {
                    0.0
                };
                let lit = 1.0 - 0.8 * shadow;
                let gc = scale(accent, l * 0.75 * lit);
                line[px] = add(add(scale(base, lit), gc), band);
                emit[px] = scale(gc, 0.55);
                floor_t[px] = t;
            }
        }
                });
            }
        });
    }

    fn put_sky(&mut self, cam: &Camera, dir: V3, c: Rgb, emit: f32) {
        let v = v3(dir.dot(cam.right), dir.dot(cam.up), dir.dot(cam.fwd));
        if v.z <= 0.05 {
            return;
        }
        let (sx, sy) = cam.project_view(v);
        if sx < 0.0 || sy < 0.0 || sx >= self.w as f32 || sy >= self.h as f32 {
            return;
        }
        let i = sy as usize * self.w + sx as usize;
        if self.floor_t[i] > 0.0 {
            return;
        }
        for k in 0..3 {
            self.color[i][k] += c[k];
            self.emit[i][k] += c[k] * emit;
        }
    }

    fn draw_stars(&mut self, scene: &Scene, cam: &Camera, t: f32) {
        let th = &scene.theme;
        for s in 0..self.stars.len() {
            let st = &self.stars[s];
            let tw = 0.55 + 0.45 * (t * st.speed + st.phase).sin();
            let tint = if st.tint < 0.2 { th.cyan } else if st.tint < 0.35 { th.magenta } else { WHITE };
            let c = scale(lerp(WHITE, tint, 0.6), st.bright * tw * 1.1);
            let dir = st.dir;
            let em = if st.bright > 0.5 { 1.5 } else { 0.0 };
            self.put_sky(cam, dir, c, em);
        }

        // A meteor every ~9 seconds, its slot chosen by a hash of the period index.
        let period = 9.0;
        let n = (t / period).floor();
        let lt = t - n * period;
        if lt < 1.2 {
            let h = |k: f32| ((n * 12.9898 + k * 78.233).sin() * 43758.547).fract().abs();
            let a = h(1.0) * TAU * 0.5 - PI * 0.75;
            let el = 0.25 + h(2.0) * 0.35;
            let start = v3(a.sin() * el.cos(), el.sin(), -a.cos() * el.cos());
            let travel = v3(0.55 + h(3.0) * 0.3, -0.35, 0.1 + h(4.0) * 0.2) * if h(5.0) < 0.5 { 1.0 } else { -1.0 };
            let head = lt / 1.2;
            let fade = (PI * head).sin();
            for k in 0..24 {
                let s = head - k as f32 * 0.012;
                if s < 0.0 {
                    break;
                }
                let dir = (start + v3(travel.x.abs() * travel.x.signum(), travel.y, travel.z) * (s * 0.6)).norm();
                let b = fade * (1.0 - k as f32 / 24.0).powi(2) * 1.6;
                self.put_sky(cam, dir, lerp(scale(WHITE, b), scale(scene.theme.cyan, b), k as f32 / 24.0), 1.2);
            }
        }
    }

    fn draw_motes(&mut self, scene: &Scene, cam: &Camera, t: f32) {
        let c = lerp(scene.theme.accent, WHITE, 0.5);
        for m in &self.motes {
            let y = (m.base.y + t * 0.8).rem_euclid(50.0);
            let p = v3(m.base.x + (t * 0.3 + m.phase).sin() * 2.0, y, m.base.z + (t * 0.23 + m.phase).cos() * 2.0);
            let v = cam.view(p);
            if v.z < 1.0 {
                continue;
            }
            let (sx, sy) = cam.project_view(v);
            if sx < 0.0 || sy < 0.0 || sx >= self.w as f32 || sy >= self.h as f32 {
                continue;
            }
            let i = sy as usize * self.w + sx as usize;
            if self.g_iz[i] > 1.0 / v.z {
                continue;
            }
            let edge = smoothstep(0.0, 6.0, y) * smoothstep(50.0, 44.0, y);
            let b = (0.35 + 0.25 * (t * 2.0 + m.phase).sin()) * edge * (60.0 / v.z).min(1.0);
            for k in 0..3 {
                self.color[i][k] += c[k] * b;
                self.emit[i][k] += c[k] * b * 0.8;
            }
        }
    }

    fn draw_voxels(&mut self, scene: &Scene, cam: &Camera, mirror: bool) {
        self.project_quads(scene, cam, mirror);
        let w = self.w;
        let h = self.h;
        let rows_per = h.div_ceil(threads());
        let quads = &self.quads;
        let bands = self
            .g_iz
            .chunks_mut(rows_per * w)
            .zip(self.g_col.chunks_mut(rows_per * w))
            .zip(self.g_glow.chunks_mut(rows_per * w))
            .enumerate();
        std::thread::scope(|sc| {
            for (band, ((iz, col), glow)) in bands {
                sc.spawn(move || {
                    iz.fill(0.0);
                    let mut target = Band { w, y0: (band * rows_per) as i32, y1: (band * rows_per + iz.len() / w) as i32, iz, col, glow };
                    for q in quads {
                        if q.y1 < target.y0 as f32 || q.y0 >= target.y1 as f32 {
                            continue;
                        }
                        target.tri(&q.v[0], &q.v[1], &q.v[2], q);
                        target.tri(&q.v[0], &q.v[2], &q.v[3], q);
                    }
                });
            }
        });
    }

    /// Project every visible voxel face into a screen-space quad.
    fn project_quads(&mut self, scene: &Scene, cam: &Camera, mirror: bool) {
        self.quads.clear();
        let l = self.light;
        let vox = &scene.voxels;
        // Outline voxels only when they are big enough on screen; deciding per
        // frame (not per face) keeps near and far faces consistent.
        let voxel_px = cam.f / (cam.pos - scene.center).len();
        let outlines = voxel_px >= 5.0;
        for v in vox {
            if v.scale < 0.02 {
                continue;
            }
            // Cheap reject: far off screen or behind the camera.
            let center = if mirror { v3(v.pos.x, -v.pos.y, v.pos.z) } else { v.pos };
            let vc = cam.view(center);
            if vc.z < 2.0 {
                continue;
            }
            let (sx, sy) = cam.project_view(vc);
            let margin = cam.f / vc.z * 2.0 + 2.0;
            if sx < -margin || sy < -margin || sx > self.w as f32 + margin || sy > self.h as f32 + margin {
                continue;
            }

            let rot = if v.rot == V3::default() { M3::IDENTITY } else { M3::euler(v.rot) };
            let hs = 0.5 * v.scale;
            for (f, &(dx, dy, dz)) in FACE_DIRS.iter().enumerate() {
                if v.at_home {
                    let nb = v.neighbors[f];
                    if nb >= 0 && vox[nb as usize].at_home {
                        continue;
                    }
                }
                let n_local = v3(dx as f32, dy as f32, dz as f32);
                let n = rot.apply(n_local);
                let fc = v.pos + n * hs;
                // Back-face cull in the (possibly mirrored) space.
                let (n_m, fc_m) = if mirror { (v3(n.x, -n.y, n.z), v3(fc.x, -fc.y, fc.z)) } else { (n, fc) };
                if n_m.dot(cam.pos - fc_m) <= 0.0 {
                    continue;
                }

                // Two in-plane axes for this face.
                let (a, b) = face_axes(n_local);
                let (a, b) = (rot.apply(a) * hs, rot.apply(b) * hs);
                let corners = [fc - a - b, fc + a - b, fc + a + b, fc - a + b];
                let uvs = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
                let mut sv = [SV { x: 0.0, y: 0.0, iz: 0.0, u: 0.0, v: 0.0 }; 4];
                let mut ok = true;
                for k in 0..4 {
                    let p = if mirror { v3(corners[k].x, -corners[k].y, corners[k].z) } else { corners[k] };
                    let vv = cam.view(p);
                    if vv.z < 0.5 {
                        ok = false;
                        break;
                    }
                    let (x, y) = cam.project_view(vv);
                    sv[k] = SV { x, y, iz: 1.0 / vv.z, u: uvs[k].0, v: uvs[k].1 };
                }
                if !ok {
                    continue;
                }

                // Flat shading: hemispheric ambient + key light + specular.
                let ndl = n.dot(l).max(0.0);
                let view_dir = (cam.pos - fc).norm();
                let hv = (l + view_dir).norm();
                let spec = n.dot(hv).max(0.0).powi(24) * 0.35;
                let amb = 0.3 + 0.14 * (n.y * 0.5 + 0.5);
                let k = amb + 0.85 * ndl;
                // A little per-voxel brightness variation keeps blocks readable.
                let k = k * (0.93 + 0.14 * v.seed[3]);
                let c = [v.color[0] * k + spec, v.color[1] * k + spec, v.color[2] * k + spec];

                let area = ((sv[2].x - sv[0].x) * (sv[3].y - sv[1].y) - (sv[3].x - sv[1].x) * (sv[2].y - sv[0].y)).abs() * 0.5;
                let px = area.sqrt();
                let edge = if outlines && px >= 2.0 { (0.9 / px).min(0.3) } else { 0.0 };
                let y0 = sv.iter().map(|p| p.y).fold(f32::MAX, f32::min);
                let y1 = sv.iter().map(|p| p.y).fold(f32::MIN, f32::max);
                self.quads.push(Quad { v: sv, col: c, glow: v.glow, edge, y0, y1 });
            }
        }
    }

}

struct Quad {
    v: [SV; 4],
    col: Rgb,
    glow: f32,
    edge: f32,
    y0: f32,
    y1: f32,
}

/// A horizontal slice of the voxel buffers, owned by one raster thread.
struct Band<'a> {
    w: usize,
    y0: i32,
    y1: i32,
    iz: &'a mut [f32],
    col: &'a mut [Rgb],
    glow: &'a mut [f32],
}

impl Band<'_> {
    fn tri(&mut self, a: &SV, b: &SV, c: &SV, q: &Quad) {
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area.abs() < 1e-5 {
            return;
        }
        let inv = 1.0 / area;
        let x0 = a.x.min(b.x).min(c.x).floor().max(0.0) as i32;
        let x1 = a.x.max(b.x).max(c.x).ceil().min(self.w as f32 - 1.0) as i32;
        let y0 = (a.y.min(b.y).min(c.y).floor() as i32).max(self.y0);
        let y1 = (a.y.max(b.y).max(c.y).ceil() as i32).min(self.y1 - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }
        // Barycentric weights are linear in x: step them along each row.
        let d0 = -(c.y - b.y) * inv;
        let d1 = -(a.y - c.y) * inv;
        let eps = -1e-4;
        let (au, av, bu, bv, cu, cv) = (a.u * a.iz, a.v * a.iz, b.u * b.iz, b.v * b.iz, c.u * c.iz, c.v * c.iz);
        for y in y0..=y1 {
            let py = y as f32 + 0.5;
            let px = x0 as f32 + 0.5;
            let mut w0 = ((c.x - b.x) * (py - b.y) - (c.y - b.y) * (px - b.x)) * inv;
            let mut w1 = ((a.x - c.x) * (py - c.y) - (a.y - c.y) * (px - c.x)) * inv;
            let row = (y - self.y0) as usize * self.w;
            for x in x0..=x1 {
                let w2 = 1.0 - w0 - w1;
                if w0 >= eps && w1 >= eps && w2 >= eps {
                    let iz = w0 * a.iz + w1 * b.iz + w2 * c.iz;
                    let i = row + x as usize;
                    if iz > self.iz[i] {
                        let mut shade = 1.0;
                        if q.edge > 0.0 {
                            let u = (w0 * au + w1 * bu + w2 * cu) / iz;
                            let v = (w0 * av + w1 * bv + w2 * cv) / iz;
                            if u.min(1.0 - u).min(v).min(1.0 - v) < q.edge {
                                shade = 0.72;
                            }
                        }
                        self.iz[i] = iz;
                        self.col[i] = scale(q.col, shade);
                        self.glow[i] = q.glow;
                    }
                }
                w0 += d0;
                w1 += d1;
            }
        }
    }
}

impl Renderer {
    fn bloom(&mut self) {
        let (w, h) = (self.w, self.h);
        let (sw, sh) = (w.div_ceil(4), h.div_ceil(4));
        for sy in 0..sh {
            for sx in 0..sw {
                let mut acc = [0.0; 3];
                let mut n = 0.0;
                for y in sy * 4..(sy * 4 + 4).min(h) {
                    for x in sx * 4..(sx * 4 + 4).min(w) {
                        let e = self.emit[y * w + x];
                        acc = [acc[0] + e[0], acc[1] + e[1], acc[2] + e[2]];
                        n += 1.0;
                    }
                }
                self.small[sy * sw + sx] = scale(acc, 1.0 / n);
            }
        }
        for _ in 0..2 {
            box_blur_rgb(&mut self.small, &mut self.small_tmp, sw, sh, 2);
        }
        let strength = 1.25;
        let rows_per = h.div_ceil(threads());
        let small = &self.small;
        std::thread::scope(|sc| {
            for (band, color) in self.color.chunks_mut(rows_per * w).enumerate() {
                sc.spawn(move || {
                    for ly in 0..color.len() / w {
                        let y = band * rows_per + ly;
                        let fy = ((y as f32 + 0.5) / 4.0 - 0.5).clamp(0.0, (sh - 1) as f32);
                        let y0 = fy as usize;
                        let y1 = (y0 + 1).min(sh - 1);
                        let ty = fy - y0 as f32;
                        for x in 0..w {
                            let fx = ((x as f32 + 0.5) / 4.0 - 0.5).clamp(0.0, (sw - 1) as f32);
                            let x0 = fx as usize;
                            let x1 = (x0 + 1).min(sw - 1);
                            let tx = fx - x0 as f32;
                            let top = lerp(small[y0 * sw + x0], small[y0 * sw + x1], tx);
                            let bot = lerp(small[y1 * sw + x0], small[y1 * sw + x1], tx);
                            let b = lerp(top, bot, ty);
                            let c = &mut color[ly * w + x];
                            for k in 0..3 {
                                c[k] += b[k] * strength;
                            }
                        }
                    }
                });
            }
        });
    }

    fn tonemap(&mut self) {
        // Separable vignette and a soft-clip tone curve, both precomputed.
        let (w, h) = (self.w as f32, self.h as f32);
        let vx: Vec<f32> = (0..self.w).map(|x| ((x as f32 + 0.5) / w * 2.0 - 1.0).powi(2) * 0.6 * 0.38).collect();
        let vy: Vec<f32> = (0..self.h).map(|y| ((y as f32 + 0.5) / h * 2.0 - 1.0).powi(2) * 0.8 * 0.38).collect();
        let lut = &self.tone_lut;
        let scale_lut = (TONE_LUT_LEN - 1) as f32 / TONE_LUT_MAX;
        let w = self.w;
        let rows_per = self.h.div_ceil(threads());
        let (vx, vy) = (&vx, &vy);
        std::thread::scope(|sc| {
            for (band, (color, out)) in self.color.chunks(rows_per * w).zip(self.out.chunks_mut(rows_per * w * 3)).enumerate() {
                sc.spawn(move || {
                    for (i, c) in color.iter().enumerate() {
                        let vig = 1.0 - vx[i % w] - vy[band * rows_per + i / w];
                        for k in 0..3 {
                            let idx = ((c[k] * vig).max(0.0) * scale_lut) as usize;
                            out[i * 3 + k] = lut[idx.min(TONE_LUT_LEN - 1)];
                        }
                    }
                });
            }
        });
    }
}

const TONE_LUT_LEN: usize = 2048;
const TONE_LUT_MAX: f32 = 4.0;

fn tone_lut() -> Vec<u8> {
    (0..TONE_LUT_LEN)
        .map(|i| {
            let v = i as f32 / (TONE_LUT_LEN - 1) as f32 * TONE_LUT_MAX;
            let v = if v < 0.78 { v } else { 0.78 + 0.22 * (1.0 - (-(v - 0.78) / 0.22).exp()) };
            (v * 255.0 + 0.5) as u8
        })
        .collect()
}

pub fn threads() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 4))
}

/// Distance-to-line intensity for an infinite grid along one axis.
fn grid_line(x: f32, spacing: f32, footprint: f32) -> f32 {
    let d = ((x / spacing).fract().abs() - 0.5).abs();
    let dist = (0.5 - d) * spacing;
    let width = 0.06 + footprint * 0.9;
    (1.0 - dist / width).max(0.0)
}

fn face_axes(n: V3) -> (V3, V3) {
    if n.x != 0.0 {
        (v3(0.0, 1.0, 0.0), v3(0.0, 0.0, 1.0))
    } else if n.y != 0.0 {
        (v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))
    } else {
        (v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0))
    }
}

fn box_blur_f32(buf: &mut [f32], tmp: &mut [f32], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    for y in 0..h {
        let row = &buf[y * w..(y + 1) * w];
        let mut acc: f32 = (0..=r).map(|k| row[k.min(w - 1)]).sum::<f32>() + row[0] * r as f32;
        for x in 0..w {
            tmp[y * w + x] = acc * norm;
            acc += row[(x + r + 1).min(w - 1)] - row[x.saturating_sub(r)];
        }
    }
    for x in 0..w {
        let mut acc: f32 = (0..=r).map(|k| tmp[k.min(h - 1) * w + x]).sum::<f32>() + tmp[x] * r as f32;
        for y in 0..h {
            buf[y * w + x] = acc * norm;
            acc += tmp[(y + r + 1).min(h - 1) * w + x] - tmp[y.saturating_sub(r) * w + x];
        }
    }
}

fn box_blur_rgb(buf: &mut [Rgb], tmp: &mut [Rgb], w: usize, h: usize, r: usize) {
    let norm = 1.0 / (2 * r + 1) as f32;
    let add = |a: Rgb, b: Rgb, s: f32| [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s];
    for y in 0..h {
        let row = &buf[y * w..(y + 1) * w];
        let mut acc = scale(row[0], r as f32);
        for k in 0..=r {
            acc = add(acc, row[k.min(w - 1)], 1.0);
        }
        for x in 0..w {
            tmp[y * w + x] = scale(acc, norm);
            acc = add(add(acc, row[(x + r + 1).min(w - 1)], 1.0), row[x.saturating_sub(r)], -1.0);
        }
    }
    for x in 0..w {
        let mut acc = scale(tmp[x], r as f32);
        for k in 0..=r {
            acc = add(acc, tmp[k.min(h - 1) * w + x], 1.0);
        }
        for y in 0..h {
            buf[y * w + x] = scale(acc, norm);
            acc = add(add(acc, tmp[(y + r + 1).min(h - 1) * w + x], 1.0), tmp[y.saturating_sub(r) * w + x], -1.0);
        }
    }
}
