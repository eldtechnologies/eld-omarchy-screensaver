use std::ops::{Add, AddAssign, Mul, Neg, Sub};

pub const TAU: f32 = std::f32::consts::TAU;
pub const PI: f32 = std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> V3 {
        let l = self.len();
        if l > 1e-9 {
            self * (1.0 / l)
        } else {
            self
        }
    }
    pub fn lerp(self, o: V3, t: f32) -> V3 {
        self + (o - self) * t
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, s: f32) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

/// Row-major 3x3 rotation matrix.
#[derive(Clone, Copy, Debug)]
pub struct M3(pub [[f32; 3]; 3]);

impl M3 {
    pub const IDENTITY: M3 = M3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    pub fn rx(a: f32) -> M3 {
        let (s, c) = a.sin_cos();
        M3([[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]])
    }
    pub fn ry(a: f32) -> M3 {
        let (s, c) = a.sin_cos();
        M3([[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]])
    }
    pub fn rz(a: f32) -> M3 {
        let (s, c) = a.sin_cos();
        M3([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]])
    }
    /// Euler angles applied X first, then Y, then Z (R = Rz * Ry * Rx).
    pub fn euler(r: V3) -> M3 {
        M3::rz(r.z) * M3::ry(r.y) * M3::rx(r.x)
    }
    /// Inverse of `euler`.
    pub fn to_euler(&self) -> V3 {
        let m = &self.0;
        let y = (-m[2][0]).clamp(-1.0, 1.0).asin();
        v3(m[2][1].atan2(m[2][2]), y, m[1][0].atan2(m[0][0]))
    }
    pub fn apply(&self, v: V3) -> V3 {
        let m = &self.0;
        v3(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        )
    }
}

impl Mul for M3 {
    type Output = M3;
    fn mul(self, o: M3) -> M3 {
        let mut r = [[0.0; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| self.0[i][k] * o.0[k][j]).sum();
            }
        }
        M3(r)
    }
}

pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

pub fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp01((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

/// Wrap an angle into [-PI, PI].
pub fn wrap_angle(a: f32) -> f32 {
    a - TAU * (a / TAU).round()
}

#[derive(Clone, Copy, Debug)]
pub enum Ease {
    InOut,
    InCubic,
    OutCubic,
    OutBounce,
}

impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        let t = clamp01(t);
        match self {
            Ease::InOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Ease::InCubic => t * t * t,
            Ease::OutCubic => 1.0 - (1.0 - t).powi(3),
            Ease::OutBounce => {
                let n1 = 7.5625;
                let d1 = 2.75;
                if t < 1.0 / d1 {
                    n1 * t * t
                } else if t < 2.0 / d1 {
                    let t = t - 1.5 / d1;
                    n1 * t * t + 0.75
                } else if t < 2.5 / d1 {
                    let t = t - 2.25 / d1;
                    n1 * t * t + 0.9375
                } else {
                    let t = t - 2.625 / d1;
                    n1 * t * t + 0.984375
                }
            }
        }
    }
}

/// xorshift64* PRNG.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545F4914F6CDD1D) >> 32) as u32
    }
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f32()
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u32() as usize) % n.max(1)
    }
}
