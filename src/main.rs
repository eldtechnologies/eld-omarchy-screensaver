//! eld-voxels — a 3D voxel screensaver for Omarchy, rendered in the terminal.

mod math;
mod png;
mod render;
mod scene;
mod term;

use std::io::{Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use render::{Camera, Renderer};
use scene::{Scene, ACTS, EMBEDDED_MASK};
use term::{Encoder, QUIT, RESIZED};

mod theme;

/// Why the main loop stopped, for --stats: 1 terminal closed, 2 focus lost, 3 input.
static QUIT_REASON: AtomicU8 = AtomicU8::new(0);

const HELP: &str = "\
eld-voxels — 3D voxel logo screensaver for Omarchy

usage: eld-voxels [options]

  --screensaver        exit on any key or mouse movement, or when the window
                       loses focus (used by the Omarchy integration)
  --fps N              target frame rate (default 30)
  --mask FILE          logo mask ('#' = voxel), see tools/png2mask.py
  --depth N            extrusion depth in voxels (default 5)
  --act NAME           start with this act: wave explode globe torus vortex galaxy helix
  --seed N             random seed
  --snapshot FILE.png  render one frame to a PNG instead of the terminal
  --at SECONDS         time of the snapshot (default 8)
  --frames N           write N snapshots at --fps spacing (FILE_0000.png, ...)
  --size WxH           pixel size for --snapshot / --bench (default 640x360)
  --aspect R           pixel height/width for --snapshot (terminals: ~1.25)
  --duration SECONDS   quit after this long
  --stats FILE         on exit, write the average frame rate to FILE
  --bench N            render N frames, write the terminal stream to stdout,
                       and print timings to stderr

keys (preview mode): n = next act, p = next palette, anything else quits
";

struct Opts {
    screensaver: bool,
    fps: f32,
    mask: String,
    depth: usize,
    act: Option<&'static str>,
    seed: u64,
    snapshot: Option<String>,
    at: f32,
    size: (usize, usize),
    aspect: f32,
    frames: usize,
    bench: Option<usize>,
    duration: Option<f32>,
    stats: Option<String>,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts {
        screensaver: false,
        fps: 30.0,
        mask: EMBEDDED_MASK.to_string(),
        depth: 5,
        act: None,
        seed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1),
        snapshot: None,
        at: 8.0,
        size: (640, 360),
        aspect: 1.0,
        frames: 1,
        bench: None,
        duration: None,
        stats: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "--screensaver" => o.screensaver = true,
            "--fps" => o.fps = val()?.parse().map_err(|_| "bad --fps")?,
            "--mask" => {
                let path = val()?;
                o.mask = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            }
            "--depth" => o.depth = val()?.parse::<usize>().map_err(|_| "bad --depth")?.clamp(1, 16),
            "--act" => {
                let name = val()?;
                o.act = Some(ACTS.iter().copied().find(|a| *a == name).ok_or(format!("unknown act {name}"))?);
            }
            "--seed" => o.seed = val()?.parse().map_err(|_| "bad --seed")?,
            "--snapshot" => o.snapshot = Some(val()?),
            "--at" => o.at = val()?.parse().map_err(|_| "bad --at")?,
            "--size" => {
                let v = val()?;
                let (w, h) = v.split_once('x').ok_or("bad --size")?;
                o.size = (w.parse().map_err(|_| "bad --size")?, h.parse().map_err(|_| "bad --size")?);
            }
            "--duration" => o.duration = Some(val()?.parse().map_err(|_| "bad --duration")?),
            "--stats" => o.stats = Some(val()?),
            "--frames" => o.frames = val()?.parse::<usize>().map_err(|_| "bad --frames")?.max(1),
            "--aspect" => o.aspect = val()?.parse().map_err(|_| "bad --aspect")?,
            "--bench" => o.bench = Some(val()?.parse().map_err(|_| "bad --bench")?),
            _ => return Err(format!("unknown option {a}\n\n{HELP}")),
        }
    }
    Ok(o)
}

fn main() {
    let o = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let mut scene = Scene::new(&o.mask, o.depth, o.seed, o.act);

    if let Some(path) = &o.snapshot {
        let (w, h) = o.size;
        let mut r = Renderer::new(w, h, o.aspect, o.seed);
        let dt = 1.0 / 30.0;
        while scene.time < o.at {
            scene.update(dt);
        }
        for n in 0..o.frames {
            if n > 0 {
                let target = scene.time + 1.0 / o.fps;
                while scene.time < target - 1e-4 {
                    scene.update((target - scene.time).min(dt));
                }
            }
            r.render(&scene, &Camera::orbit(&scene, scene.time, w, h, o.aspect));
            let file = if o.frames > 1 { format!("{}_{n:04}.png", path.trim_end_matches(".png")) } else { path.clone() };
            if let Err(e) = png::write(&file, w, h, &r.out) {
                eprintln!("{file}: {e}");
                std::process::exit(1);
            }
        }
        eprintln!("wrote {} frame(s) to {path} (t={:.1}s, act={})", o.frames, scene.time, scene.act_name);
        return;
    }

    if let Some(frames) = o.bench {
        let (w, h) = o.size;
        let mut r = Renderer::new(w, h, 1.0, o.seed);
        let mut enc = Encoder::new(w, h / 2);
        let mut out = std::io::stdout().lock();
        let (mut render_t, mut bytes) = (Duration::ZERO, 0usize);
        let start = Instant::now();
        for _ in 0..frames {
            scene.update(1.0 / o.fps);
            let t0 = Instant::now();
            r.render(&scene, &Camera::orbit(&scene, scene.time, w, h, 1.0));
            render_t += t0.elapsed();
            let data = enc.encode(&r.out, h);
            bytes += data.len();
            out.write_all(&data).ok();
        }
        out.flush().ok();
        let n = frames.max(1) as f64;
        eprintln!(
            "{frames} frames {w}x{h}: render {:.2} ms/frame, total {:.2} ms/frame, {:.0} KiB/frame",
            render_t.as_secs_f64() * 1000.0 / n,
            start.elapsed().as_secs_f64() * 1000.0 / n,
            bytes as f64 / n / 1024.0
        );
        let names = ["shadow", "sky+floor", "reflection", "voxels", "bloom", "tonemap"];
        let stages: Vec<String> = names.iter().zip(r.timings).map(|(k, v)| format!("{k} {:.2}", v * 1000.0 / n)).collect();
        eprintln!("  stages (ms): {}", stages.join(", "));
        return;
    }

    run(o, scene);
}

fn run(o: Opts, mut scene: Scene) {
    term::install_signal_handlers();
    let raw = term::RawMode::enable();

    // A writer thread owns stdout, so the next frame renders while the
    // terminal is still chewing on the previous one.
    let (frame_tx, frame_rx) = mpsc::sync_channel::<Vec<u8>>(1);
    let writer = std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        for data in frame_rx {
            if out.write_all(&data).and_then(|_| out.flush()).is_err() {
                QUIT_REASON.store(1, Ordering::SeqCst);
                QUIT.store(true, Ordering::SeqCst);
                break;
            }
        }
    });
    let send = |data: Vec<u8>| frame_tx.send(data).is_ok();
    send([term::ENTER, if o.screensaver { term::MOUSE_ON } else { "" }].concat().into_bytes());

    // Keyboard and mouse input arrive on the tty.
    let (tx, rx) = mpsc::channel::<u8>();
    std::thread::spawn(move || {
        if let Ok(mut tty) = std::fs::File::open("/dev/tty") {
            let mut b = [0u8; 64];
            while let Ok(n) = tty.read(&mut b) {
                if n == 0 {
                    break;
                }
                for &c in &b[..n] {
                    if tx.send(c).is_err() {
                        return;
                    }
                }
            }
        }
    });

    if o.screensaver {
        // Like the stock Omarchy screensaver: quit once focus moves elsewhere.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(2));
            loop {
                if let Ok(res) = Command::new("hyprctl").args(["activewindow", "-j"]).output() {
                    if res.status.success() && !String::from_utf8_lossy(&res.stdout).contains("org.omarchy.screensaver") {
                        QUIT_REASON.store(2, Ordering::SeqCst);
                        QUIT.store(true, Ordering::SeqCst);
                        return;
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }

    let started = Instant::now();
    let frame = Duration::from_secs_f32(1.0 / o.fps.max(1.0));
    let mut state: Option<(Renderer, Encoder, usize, usize)> = None;
    let mut last = Instant::now();
    let mut check_size = 0;
    let mut frames = 0u64;
    let mut stream_bytes = 0usize;
    let (mut busy_render, mut busy_write) = (Duration::ZERO, Duration::ZERO);
    let mut last_input = 0u8;

    while !QUIT.load(Ordering::SeqCst) {
        let t0 = Instant::now();

        while let Ok(c) = rx.try_recv() {
            // Ignore input during the first moment, while the window maps.
            if started.elapsed() < Duration::from_millis(800) {
                continue;
            }
            match c {
                b'n' if !o.screensaver => scene.skip_act(),
                b'p' if !o.screensaver => scene.next_palette(),
                _ => {
                    QUIT_REASON.store(3, Ordering::SeqCst);
                    last_input = c;
                    QUIT.store(true, Ordering::SeqCst)
                }
            }
        }

        check_size += 1;
        if state.is_none() || RESIZED.swap(false, Ordering::SeqCst) || check_size % 60 == 0 {
            let s = term::size().unwrap_or(term::Size { cols: 80, rows: 24, pixel_aspect: 1.0 });
            let changed = state.as_ref().is_none_or(|(_, _, c, r)| *c != s.cols || *r != s.rows);
            if changed {
                let (w, h) = (s.cols, s.rows * 2);
                state = Some((Renderer::new(w, h, s.pixel_aspect, o.seed), Encoder::new(s.cols, s.rows), s.cols, s.rows));
                send(b"\x1b[0m\x1b[2J".to_vec());
            }
        }
        let (r, enc, _, _) = state.as_mut().unwrap();

        let dt = last.elapsed().as_secs_f32().min(0.1);
        last = Instant::now();
        scene.update(dt);
        let cam = Camera::orbit(&scene, scene.time, r.w, r.h, r.aspect);
        r.render(&scene, &cam);
        let data = enc.encode(&r.out, r.h);
        stream_bytes += data.len();
        busy_render += t0.elapsed();
        let tw = Instant::now();
        if !send(data) {
            break;
        }
        busy_write += tw.elapsed();
        frames += 1;
        if o.duration.is_some_and(|d| started.elapsed().as_secs_f32() >= d) {
            break;
        }

        if let Some(rest) = frame.checked_sub(t0.elapsed()) {
            std::thread::sleep(rest);
        }
    }

    send([if o.screensaver { term::MOUSE_OFF } else { "" }, term::LEAVE].concat().into_bytes());
    drop(frame_tx);
    let _ = writer.join();
    drop(raw);

    if let Some(path) = &o.stats {
        let secs = started.elapsed().as_secs_f64();
        let (cols, rows) = state.as_ref().map(|s| (s.2, s.3)).unwrap_or_default();
        let line = format!(
            "{frames} frames in {secs:.1}s = {:.1} fps, {cols}x{rows} cells, {:.0} KiB/frame, render {:.1} ms, blocked on terminal {:.1} ms, exit: {}\n",
            frames as f64 / secs,
            stream_bytes as f64 / frames.max(1) as f64 / 1024.0,
            busy_render.as_secs_f64() * 1000.0 / frames.max(1) as f64,
            busy_write.as_secs_f64() * 1000.0 / frames.max(1) as f64,
            match QUIT_REASON.load(Ordering::SeqCst) {
                1 => "terminal closed".to_string(),
                2 => "focus lost".to_string(),
                3 => format!("input byte {last_input:#04x}"),
                _ if QUIT.load(Ordering::SeqCst) => "signal".to_string(),
                _ => "duration".to_string(),
            }
        );
        let _ = std::fs::write(path, line);
    }
}
