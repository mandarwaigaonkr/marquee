//! Background worker that grabs frames from the user's videos. It runs two extra
//! mpv instances of its own, apart from the player:
//!
//! - **Thumbnails**: one frame per file (plus its duration and size), made whenever
//!   nothing is playing. This one renders through the GPU into a window that is never
//!   shown, so HDR files are tone-mapped and look right.
//! - **Scrubber previews**: small frames on request, including during playback. This
//!   one decodes on the CPU and writes images straight to disk. It must not touch the
//!   GPU: a second GPU renderer made a 4K file drop about fifty frames per scrub.

use crate::{db::Db, player::Player};
use libmpv2::{events::Event, Mpv};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Mutex},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

const IDLE_WAIT: Duration = Duration::from_millis(1500);
/// How often to look again for files whose drive is not connected.
const MISSING_WAIT: Duration = Duration::from_secs(10);
/// A file opened for scrubber previews is let go after this long without a request,
/// so the app does not sit on a file the user may want to move.
const PREVIEW_LINGER: Duration = Duration::from_secs(20);

struct PreviewRequest {
    path: String,
    seconds: f64,
    out: PathBuf,
    reply: mpsc::Sender<bool>,
}

/// Asks the worker for scrubber frames.
pub struct Previews(Mutex<mpsc::Sender<PreviewRequest>>);

impl Previews {
    /// Writes the frame nearest `seconds` of the video at `path` to `out`. False when
    /// it could not be made, or when a newer request replaced this one.
    pub fn frame(&self, path: &str, seconds: f64, out: &Path) -> bool {
        let (reply, done) = mpsc::channel();
        let request = PreviewRequest { path: path.to_owned(), seconds, out: out.to_owned(), reply };
        if self.0.lock().unwrap().send(request).is_err() {
            return false;
        }
        done.recv_timeout(Duration::from_secs(5)).unwrap_or(false)
    }
}

/// Lets go of whatever is loaded and waits for mpv to say so. The end-of-file event
/// arrives a moment after `stop` is asked for.
fn close(mpv: &Mpv) {
    let _ = mpv.command("stop", &[]);
    for _ in 0..40 {
        if mpv.get_property::<bool>("idle-active").unwrap_or(true) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    drain(mpv);
}

fn drain(mpv: &Mpv) {
    while mpv.wait_event(0.0).is_some() {}
}

/// Waits until a frame has been shown. After a load, an end-of-file only counts once
/// this load's own start has arrived: before that it belongs to the file before it.
fn await_frame(mpv: &Mpv, fresh_load: bool, tenths: u32) -> bool {
    let mut started = !fresh_load;
    for _ in 0..tenths {
        match mpv.wait_event(0.1) {
            Some(Ok(Event::StartFile)) => started = true,
            Some(Ok(Event::PlaybackRestart)) if started => return true,
            // The file could not be opened or decoded: give up now rather than wait out the clock.
            Some(Ok(Event::EndFile(_))) | Some(Err(_)) if started => return false,
            _ => {}
        }
    }
    false
}

fn thumbnailer(hidden_hwnd: i64) -> Result<Mpv, libmpv2::Error> {
    Mpv::with_initializer(|init| {
        init.set_option("wid", hidden_hwnd)?;
        init.set_option("vo", "gpu-next")?;
        init.set_option("ao", "null")?;
        init.set_option("pause", "yes")?;
        init.set_option("idle", "yes")?;
        init.set_option("force-window", "yes")?;
        init.set_option("hwdec", "auto-copy")?;
        init.set_option("vf", "scale=w=960:h=-2")?;
        init.set_option("sid", "no")?;
        init.set_option("aid", "no")?;
        init.set_option("hr-seek", "yes")?;
        // A tenth of the way in skips studio logos and cold opens to black.
        init.set_option("start", "10%")?;
        init.set_option("screenshot-format", "jpg")?;
        init.set_option("screenshot-jpeg-quality", "80")?;
        Ok(())
    })
}

/// One frame for the library, with the file's (duration, width, height).
fn thumbnail(mpv: &Mpv, path: &str, out: &Path) -> Option<(i64, i64, i64)> {
    close(mpv);
    mpv.command("loadfile", &[path]).ok()?;
    let details = await_frame(mpv, true, 300).then(|| {
        thread::sleep(Duration::from_millis(120));
        let shot = mpv.command("screenshot-to-file", &[&out.to_string_lossy(), "video"]).is_ok() && out.exists();
        let duration = mpv.get_property::<f64>("duration").unwrap_or(0.0);
        let width = mpv.get_property::<i64>("video-params/w").unwrap_or(0);
        let height = mpv.get_property::<i64>("video-params/h").unwrap_or(0);
        shot.then_some(((duration * 1000.0) as i64, width, height))
    });
    close(mpv);
    details.flatten()
}

/// The scrubber's frame maker: CPU decoding, images written to `scratch`.
struct Previewer {
    mpv: Mpv,
    scratch: PathBuf,
    /// File kept open between requests, so the next frame is one seek away.
    open: Option<String>,
    last_used: Instant,
}

impl Previewer {
    fn new(scratch: PathBuf) -> Option<Self> {
        fs::create_dir_all(&scratch).ok()?;
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("vo", "image")?;
            init.set_option("vo-image-format", "jpg")?;
            init.set_option("vo-image-jpeg-quality", 75i64)?;
            init.set_option("vo-image-outdir", scratch.to_string_lossy().as_ref())?;
            init.set_option("ao", "null")?;
            init.set_option("pause", "yes")?;
            init.set_option("idle", "yes")?;
            init.set_option("hwdec", "no")?;
            // Two threads are plenty for one keyframe and leave the rest to the player.
            init.set_option("vd-lavc-threads", 2i64)?;
            init.set_option("vf", "scale=w=320:h=-2")?;
            init.set_option("sid", "no")?;
            init.set_option("aid", "no")?;
            // Keyframe seeks: fast matters more than exact.
            init.set_option("hr-seek", "no")?;
            Ok(())
        })
        .ok()?;
        Some(Self { mpv, scratch, open: None, last_used: Instant::now() })
    }

    fn clear_scratch(&self) {
        for entry in fs::read_dir(&self.scratch).into_iter().flatten().flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }

    fn close(&mut self) {
        self.open = None;
        close(&self.mpv);
        self.clear_scratch();
    }

    fn frame(&mut self, request: &PreviewRequest) -> bool {
        self.last_used = Instant::now();
        self.clear_scratch();
        let at = format!("{:.1}", request.seconds);
        let shown = if self.open.as_deref() == Some(request.path.as_str()) {
            drain(&self.mpv);
            self.mpv.command("seek", &[&at, "absolute+keyframes"]).is_ok() && await_frame(&self.mpv, false, 40)
        } else {
            self.close();
            let _ = self.mpv.set_property("start", at.as_str());
            let loaded = self.mpv.command("loadfile", &[&request.path]).is_ok() && await_frame(&self.mpv, true, 60);
            if loaded {
                self.open = Some(request.path.clone());
            }
            loaded
        };
        if !shown {
            return false;
        }
        // The image is written just after the frame is announced.
        for _ in 0..20 {
            let newest = fs::read_dir(&self.scratch).into_iter().flatten().flatten().map(|e| e.path()).max();
            if let Some(image) = newest {
                thread::sleep(Duration::from_millis(30));
                return fs::rename(&image, &request.out).or_else(|_| fs::copy(&image, &request.out).map(|_| ())).is_ok();
            }
            thread::sleep(Duration::from_millis(25));
        }
        false
    }

    /// Serves requests for up to `wait`, then returns.
    fn serve(&mut self, requests: &mpsc::Receiver<PreviewRequest>, wait: Duration) {
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match requests.recv_timeout(left) {
                Ok(mut request) => {
                    // The pointer has moved on: only the newest request is worth a seek.
                    while let Ok(newer) = requests.try_recv() {
                        let _ = request.reply.send(false);
                        request = newer;
                    }
                    let ok = self.frame(&request);
                    let _ = request.reply.send(ok);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    thread::sleep(left);
                    break;
                }
            }
        }
        if self.open.is_some() && self.last_used.elapsed() > PREVIEW_LINGER {
            self.close();
        }
    }
}

/// `scratch` is a folder of the app's own for frames in the making.
pub fn spawn(app: AppHandle, hidden_hwnd: i64, scratch: PathBuf) -> Previews {
    let (sender, requests) = mpsc::channel::<PreviewRequest>();
    thread::spawn(move || {
        let Ok(mpv) = thumbnailer(hidden_hwnd) else { return };
        let mut previewer = Previewer::new(scratch);
        let mut rest = |wait: Duration| match previewer.as_mut() {
            Some(p) => p.serve(&requests, wait),
            None => thread::sleep(wait),
        };
        // Walks the queue by id, so a file that can't be done right now doesn't block the ones after it.
        let (mut after, mut missing) = (0, false);
        loop {
            rest(Duration::ZERO);
            let db = app.state::<Db>();
            // Thumbnails never compete with playback for the disk or the decoder.
            if app.state::<Player>().current().is_some() {
                rest(IDLE_WAIT);
                continue;
            }
            let Some((id, path)) = db.next_unprobed(after) else {
                rest(if missing { MISSING_WAIT } else { IDLE_WAIT });
                (after, missing) = (0, false);
                continue;
            };
            after = id;
            // An unplugged drive is not a failure: leave the file queued and come back to it.
            if !Path::new(&path).exists() {
                missing = true;
                continue;
            }
            match thumbnail(&mpv, &path, &db.thumb_path(id)) {
                Some((duration_ms, width, height)) => db.set_probed(id, true, duration_ms, width, height),
                None => db.set_probed(id, false, 0, 0, 0),
            }
            let _ = app.emit("library-changed", ());
        }
    });
    Previews(Mutex::new(sender))
}
