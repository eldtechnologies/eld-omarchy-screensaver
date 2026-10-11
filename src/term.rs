//! Terminal plumbing: window size, raw input, signals, and a diffing
//! truecolor half-block encoder (two pixels per cell).

use std::fs::File;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

pub static QUIT: AtomicBool = AtomicBool::new(false);
pub static RESIZED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Default)]
struct WinSize {
    rows: u16,
    cols: u16,
    xpix: u16,
    ypix: u16,
}

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
}

const TIOCGWINSZ: u64 = 0x5413;

pub struct Size {
    pub cols: usize,
    pub rows: usize,
    /// Height / width of one half-block pixel.
    pub pixel_aspect: f32,
}

pub fn size() -> Option<Size> {
    let mut ws = WinSize::default();
    // SAFETY: TIOCGWINSZ writes a `struct winsize` into the pointer we pass.
    let r = unsafe { ioctl(1, TIOCGWINSZ, &mut ws as *mut WinSize) };
    if r != 0 || ws.cols == 0 || ws.rows == 0 {
        return None;
    }
    let pixel_aspect = if ws.xpix > 0 && ws.ypix > 0 {
        (ws.ypix as f32 / (ws.rows as f32 * 2.0)) / (ws.xpix as f32 / ws.cols as f32)
    } else {
        1.0
    };
    Some(Size { cols: ws.cols as usize, rows: ws.rows as usize, pixel_aspect })
}

extern "C" fn on_quit(_: i32) {
    QUIT.store(true, Ordering::SeqCst);
}

extern "C" fn on_winch(_: i32) {
    RESIZED.store(true, Ordering::SeqCst);
}

pub fn install_signal_handlers() {
    // SAFETY: the handlers only store to atomics, which is async-signal-safe.
    unsafe {
        for sig in [1, 2, 3, 15] {
            signal(sig, on_quit as extern "C" fn(i32) as usize);
        }
        signal(28, on_winch as extern "C" fn(i32) as usize);
    }
}

fn stty(args: &[&str]) -> Option<String> {
    let tty = File::open("/dev/tty").ok()?;
    let out = Command::new("stty").args(args).stdin(tty).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Puts the tty into raw-ish mode for the lifetime of the guard.
pub struct RawMode {
    saved: Option<String>,
}

impl RawMode {
    pub fn enable() -> RawMode {
        let saved = stty(&["-g"]);
        stty(&["-icanon", "-echo", "-isig", "min", "1", "time", "0"]);
        RawMode { saved }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(s) = &self.saved {
            stty(&[s.as_str()]);
        }
    }
}

pub const ENTER: &str = "\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[0m\x1b[2J";
pub const LEAVE: &str = "\x1b[0m\x1b[?7h\x1b[?25h\x1b[?1049l";
pub const MOUSE_ON: &str = "\x1b[?1003h\x1b[?1006h";
pub const MOUSE_OFF: &str = "\x1b[?1003l\x1b[?1006l";

pub struct Encoder {
    prev: Vec<u64>,
    cols: usize,
    rows: usize,
}

const UPPER_HALF: &[u8] = "▀".as_bytes();

impl Encoder {
    pub fn new(cols: usize, rows: usize) -> Encoder {
        Encoder { prev: vec![u64::MAX; cols * rows], cols, rows }
    }

    /// Encode an RGB image of `cols` x `h` pixels (two per cell), emitting only
    /// cells that changed. Bands of rows are encoded in parallel.
    pub fn encode(&mut self, rgb: &[u8], h: usize) -> Vec<u8> {
        let (cols, rows) = (self.cols, self.rows);
        let rows_per = rows.div_ceil(crate::render::threads()).max(1);
        let parts: Vec<Vec<u8>> = std::thread::scope(|sc| {
            let handles: Vec<_> = self
                .prev
                .chunks_mut(rows_per * cols)
                .enumerate()
                .map(|(band, prev)| sc.spawn(move || encode_band(rgb, cols, h, band * rows_per, prev)))
                .collect();
            handles.into_iter().map(|t| t.join().unwrap()).collect()
        });
        let mut out = Vec::with_capacity(parts.iter().map(Vec::len).sum::<usize>() + 16);
        out.extend_from_slice(b"\x1b[?2026h");
        for p in parts {
            out.extend_from_slice(&p);
        }
        out.extend_from_slice(b"\x1b[?2026l");
        out
    }
}

fn encode_band(rgb: &[u8], w: usize, h: usize, first_row: usize, prev: &mut [u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prev.len() * 8);
    let px = |x: usize, y: usize| -> u32 {
        let i = (y.min(h - 1) * w + x) * 3;
        // Drop the lowest bit: invisible, but it stops tiny gradient
        // flicker from forcing a cell rewrite.
        ((rgb[i] as u32 & 0xFE) << 16) | ((rgb[i + 1] as u32 & 0xFE) << 8) | (rgb[i + 2] as u32 & 0xFE)
    };
    let mut fg = u32::MAX;
    let mut bg = u32::MAX;
    let mut cursor = usize::MAX;
    for (idx, slot) in prev.iter_mut().enumerate() {
        let (r, c) = (first_row + idx / w, idx % w);
        let top = px(c, r * 2);
        let bot = px(c, r * 2 + 1);
        let key = ((top as u64) << 32) | bot as u64;
        if *slot == key {
            continue;
        }
        *slot = key;
        if cursor != idx {
            out.extend_from_slice(b"\x1b[");
            push_num(&mut out, r + 1);
            out.push(b';');
            push_num(&mut out, c + 1);
            out.push(b'H');
        }
        if top == bot {
            if bg != top {
                out.extend_from_slice(b"\x1b[48;2;");
                push_rgb(&mut out, top);
                out.push(b'm');
                bg = top;
            }
            out.push(b' ');
        } else {
            let set_fg = fg != top;
            let set_bg = bg != bot;
            if set_fg || set_bg {
                out.extend_from_slice(b"\x1b[");
                if set_fg {
                    out.extend_from_slice(b"38;2;");
                    push_rgb(&mut out, top);
                }
                if set_bg {
                    if set_fg {
                        out.push(b';');
                    }
                    out.extend_from_slice(b"48;2;");
                    push_rgb(&mut out, bot);
                }
                out.push(b'm');
                fg = top;
                bg = bot;
            }
            out.extend_from_slice(UPPER_HALF);
        }
        cursor = if c + 1 < w { idx + 1 } else { usize::MAX };
    }
    out
}

fn push_num(out: &mut Vec<u8>, n: usize) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut n = n;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&buf[i..]);
}

fn push_rgb(out: &mut Vec<u8>, c: u32) {
    push_num(out, (c >> 16) as usize & 255);
    out.push(b';');
    push_num(out, (c >> 8) as usize & 255);
    out.push(b';');
    push_num(out, c as usize & 255);
}
