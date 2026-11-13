//! Thin wrapper around libmpv. mpv renders into a native child window of the
//! main window (`wid`), behind the transparent webview that draws the controls.

use libmpv2::Mpv;
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

pub struct Player {
    mpv: Mpv,
    /// Media file being played and its path, so progress is saved against the right file.
    current: Mutex<Option<(i64, String)>>,
    /// Counts play and stop requests. Play looks the file up off the main thread, so
    /// by the time it is ready to load, a newer request may have replaced it.
    turn: AtomicU64,
}

/// A path in the form mpv reports it: absolute, with backslashes.
fn comparable(path: &str) -> String {
    std::path::absolute(path).map_or_else(|_| path.to_owned(), |p| p.to_string_lossy().into_owned()).replace('/', "\\")
}

#[derive(Serialize)]
pub struct Track {
    id: i64,
    kind: String,
    lang: String,
    title: String,
    codec: String,
    selected: bool,
    external: bool,
}

#[derive(Serialize)]
pub struct PlayerState {
    media_file_id: Option<i64>,
    time_pos: f64,
    duration: f64,
    paused: bool,
    idle: bool,
    ended: bool,
    volume: f64,
    sub_delay: f64,
    sub_scale: f64,
    sub_pos: f64,
    speed: f64,
    hwdec: String,
    video_codec: String,
    width: i64,
    height: i64,
    dropped_frames: i64,
    tracks: Vec<Track>,
}

type Res<T> = Result<T, String>;
fn err(e: libmpv2::Error) -> String {
    e.to_string()
}

impl Player {
    pub fn new(parent_hwnd: i64) -> Res<Self> {
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("wid", parent_hwnd)?;
            init.set_option("vo", "gpu-next")?;
            init.set_option("hwdec", "auto-safe")?;
            init.set_option("idle", "yes")?;
            init.set_option("keep-open", "yes")?;
            init.set_option("force-window", "yes")?;
            // The webview owns all input and UI; mpv only renders.
            init.set_option("osc", "no")?;
            init.set_option("input-default-bindings", "no")?;
            init.set_option("input-vo-keyboard", "no")?;
            init.set_option("input-cursor", "no")?;
            // Pick up subtitle files next to the video or in a Subs folder.
            init.set_option("sub-auto", "fuzzy")?;
            init.set_option("sub-file-paths", "Subs;subs;Subtitles;subtitles")?;
            Ok(())
        })
        .map_err(err)?;
        Ok(Self { mpv, current: Mutex::new(None), turn: AtomicU64::new(0) })
    }

    pub fn current(&self) -> Option<i64> {
        self.current.lock().unwrap().as_ref().map(|(id, _)| *id)
    }

    /// Call first thing in a play request; hand the number to `load`.
    pub fn begin(&self) -> u64 {
        self.turn.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Returns false, and does nothing, when a newer play or a stop came in since `begin`.
    /// `tracks` is the remembered (audio language, subtitle language) for the series;
    /// a subtitle language of "no" keeps subtitles off.
    pub fn load(&self, turn: u64, media_file_id: i64, path: &str, start_ms: i64, tracks: (Option<String>, Option<String>)) -> Res<bool> {
        if !Path::new(path).exists() {
            return Err(format!(
                "This file is no longer at {path}. It may have been moved or renamed, or its drive is unplugged. Put it back or reconnect the drive, then play it again."
            ));
        }
        // Held across the mpv calls so a stop or another load can't land in the middle.
        let mut current = self.current.lock().unwrap();
        if self.turn.load(Ordering::SeqCst) != turn {
            return Ok(false);
        }
        self.mpv.set_property("start", format!("{:.3}", start_ms as f64 / 1000.0)).map_err(err)?;
        // A track picked by number in the last file means nothing in this one: choose
        // by language again, falling back to mpv's own pick.
        let (audio, sub) = tracks;
        let subs_off = sub.as_deref() == Some("no");
        self.mpv.set_property("alang", audio.unwrap_or_default()).map_err(err)?;
        self.mpv.set_property("slang", if subs_off { String::new() } else { sub.unwrap_or_default() }).map_err(err)?;
        self.mpv.set_property("aid", "auto").map_err(err)?;
        self.mpv.set_property("sid", if subs_off { "no" } else { "auto" }).map_err(err)?;
        self.mpv.command("loadfile", &[path]).map_err(err)?;
        self.mpv.set_property("pause", false).map_err(err)?;
        *current = Some((media_file_id, comparable(path)));
        Ok(true)
    }

    pub fn stop(&self) -> Res<()> {
        let mut current = self.current.lock().unwrap();
        self.turn.fetch_add(1, Ordering::SeqCst);
        *current = None;
        self.mpv.command("stop", &[]).map_err(err)
    }

    pub fn toggle_pause(&self) -> Res<()> {
        // At the end of a file (kept open), play again from the start.
        if self.flag("eof-reached") {
            self.mpv.command("seek", &["0", "absolute"]).map_err(err)?;
            return self.mpv.set_property("pause", false).map_err(err);
        }
        self.mpv.command("cycle", &["pause"]).map_err(err)
    }

    pub fn seek(&self, seconds: f64, absolute: bool) -> Res<()> {
        let mode = if absolute { "absolute" } else { "relative" };
        self.mpv.command("seek", &[&seconds.to_string(), mode]).map_err(err)
    }

    /// `kind` is "audio" or "sub"; `None` turns the track off.
    pub fn set_track(&self, kind: &str, id: Option<i64>) -> Res<()> {
        let property = match kind {
            "audio" => "aid",
            "sub" => "sid",
            _ => return Err(format!("Unknown track type: {kind}")),
        };
        let value = id.map_or("no".to_string(), |i| i.to_string());
        self.mpv.set_property(property, value).map_err(err)
    }

    /// Language tag of a track of the loaded file, if it has one.
    pub fn track_lang(&self, kind: &str, id: i64) -> Option<String> {
        let count = self.mpv.get_property::<i64>("track-list/count").unwrap_or(0);
        (0..count)
            .find(|n| {
                self.mpv.get_property::<String>(&format!("track-list/{n}/type")).is_ok_and(|t| t == kind)
                    && self.mpv.get_property::<i64>(&format!("track-list/{n}/id")).is_ok_and(|i| i == id)
            })
            .and_then(|n| self.mpv.get_property::<String>(&format!("track-list/{n}/lang")).ok())
            .filter(|lang| !lang.is_empty())
    }

    pub fn set_number(&self, property: &str, value: f64) -> Res<()> {
        if !["volume", "sub-delay", "sub-scale", "sub-pos", "speed"].contains(&property) {
            return Err(format!("Unknown setting: {property}"));
        }
        self.mpv.set_property(property, value).map_err(err)
    }

    pub fn add_subtitle(&self, path: &str) -> Res<()> {
        self.mpv.command("sub-add", &[path, "select"]).map_err(err)
    }

    fn flag(&self, name: &str) -> bool {
        self.mpv.get_property::<bool>(name).unwrap_or(false)
    }

    /// (media file, position, duration) in milliseconds, when something is loaded.
    /// mpv switches files a moment after it is asked to, so the reading only counts
    /// once mpv reports the file we think is playing. Otherwise the last seconds of
    /// one episode could be saved as the position of the next.
    pub fn progress(&self) -> Option<(i64, i64, i64)> {
        let current = self.current.lock().unwrap();
        let (id, path) = current.as_ref()?;
        if !comparable(&self.mpv.get_property::<String>("path").ok()?).eq_ignore_ascii_case(path) {
            return None;
        }
        let position = self.mpv.get_property::<f64>("time-pos").ok()?;
        let duration = self.mpv.get_property::<f64>("duration").ok()?;
        Some((*id, (position * 1000.0) as i64, (duration * 1000.0) as i64))
    }

    pub fn state(&self) -> PlayerState {
        let f = |name: &str| self.mpv.get_property::<f64>(name).unwrap_or(0.0);
        let i = |name: &str| self.mpv.get_property::<i64>(name).unwrap_or(0);
        let s = |name: &str| self.mpv.get_property::<String>(name).unwrap_or_default();
        let tracks = (0..i("track-list/count"))
            .map(|n| Track {
                id: i(&format!("track-list/{n}/id")),
                kind: s(&format!("track-list/{n}/type")),
                lang: s(&format!("track-list/{n}/lang")),
                title: s(&format!("track-list/{n}/title")),
                codec: s(&format!("track-list/{n}/codec")),
                selected: self.flag(&format!("track-list/{n}/selected")),
                external: self.flag(&format!("track-list/{n}/external")),
            })
            .filter(|t| t.kind == "audio" || t.kind == "sub")
            .collect();
        PlayerState {
            media_file_id: self.current(),
            time_pos: f("time-pos"),
            duration: f("duration"),
            paused: self.flag("pause"),
            idle: self.flag("idle-active"),
            ended: self.flag("eof-reached"),
            volume: f("volume"),
            sub_delay: f("sub-delay"),
            sub_scale: self.mpv.get_property::<f64>("sub-scale").unwrap_or(1.0),
            sub_pos: self.mpv.get_property::<f64>("sub-pos").unwrap_or(100.0),
            speed: self.mpv.get_property::<f64>("speed").unwrap_or(1.0),
            hwdec: s("hwdec-current"),
            video_codec: s("video-codec"),
            width: i("width"),
            height: i("height"),
            dropped_frames: i("frame-drop-count") + i("decoder-frame-drop-count"),
            tracks,
        }
    }
}
