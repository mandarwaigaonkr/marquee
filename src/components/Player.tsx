import { useCallback, useEffect, useRef, useState } from "react";
import { type Library, type PlayerState, type Track, api, clock, episodesOf, pickSubtitle, runtime, seasonName, thumb } from "../api";
import { Icon, IconButton, Thumb } from "./ui";

const SPEEDS = [1, 1.25, 1.5, 2, 0.75];
const IDLE_MS = 2600;
/// Once the pointer has left the window the controls go sooner.
const AWAY_MS = 600;
const NEXT_COUNTDOWN = 8;
/// Scrubber frames are made every ten seconds of video (PREVIEW_STEP_SECONDS in lib.rs).
const FRAME_STEP = 10;

const languages = new Intl.DisplayNames(["en"], { type: "language" });
function trackName(t: Track, index: number) {
  let lang = "";
  try {
    lang = t.lang ? (languages.of(t.lang) ?? t.lang) : "";
  } catch {
    lang = t.lang;
  }
  // Skip a title that only repeats the language.
  const title = t.title && t.title.toLowerCase() !== lang.toLowerCase() ? t.title : "";
  return [lang, title].filter(Boolean).join(", ") || `Track ${index + 1}`;
}
const trackMeta = (t: Track) => [t.codec.toUpperCase().replace("SUBRIP", "SRT").replace("HDMV_PGS_SUBTITLE", "PGS"), t.external && "file"].filter(Boolean).join(", ");

function find(lib: Library, mediaFileId: number) {
  for (const series of lib.series) {
    const all = episodesOf(series);
    const index = all.findIndex((e) => e.media_file_id === mediaFileId);
    if (index >= 0) return { series, episode: all[index], next: all[index + 1], movie: undefined };
  }
  return { series: undefined, episode: undefined, next: undefined, movie: lib.movies.find((m) => m.media_file_id === mediaFileId) };
}

export function Player({ lib, mediaFileId, onPlay, onClose, onError }: { lib: Library; mediaFileId: number; onPlay: (mediaFileId: number) => void; onClose: () => void; onError: (message: string) => void }) {
  const [state, setState] = useState<PlayerState | null>(null);
  const [idle, setIdle] = useState(false);
  const [panel, setPanel] = useState<"tracks" | "episodes" | null>(null);
  const [stats, setStats] = useState(false);
  // Kept after the pointer leaves, so the preview can fade out where it was.
  const [peek, setPeek] = useState<{ x: number; t: number; part: number; on: boolean } | null>(null);
  // Where the user is dragging to. Shown until mpv has caught up, so the bar does not jump back.
  const [drag, setDrag] = useState<{ part: number; held: boolean } | null>(null);
  // The volume being dragged to, shown at once instead of waiting for the next reading.
  const [volumeHeld, setVolumeHeld] = useState<number | null>(null);
  const [flash, setFlash] = useState<{ key: number; icon: string; text: string } | null>(null);
  // Pointer resting on the controls keeps them up; pointer out of the window lets them go early.
  const hold = useRef(false);
  const away = useRef(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [skipNext, setSkipNext] = useState(false);
  const [frame, setFrame] = useState<{ src: string; step: number } | null>(null);
  // One frame request at a time; while it runs, only the newest position is kept.
  const frames = useRef<{ busy: boolean; want: number | null; last: number | null }>({ busy: false, want: null, last: null });
  const idleTimer = useRef<number>(undefined);
  const root = useRef<HTMLDivElement>(null);
  const lastPanel = useRef<typeof panel>(null);
  // The poll is up to 250 ms old. Stepping from the last value sent lets quick presses add up.
  const sent = useRef<Record<string, number>>({});
  const lastVolume = useRef(100);
  const { series, episode, next, movie } = find(lib, mediaFileId);
  const season = series?.seasons.find((s) => s.number === episode?.season_number);
  const call = useCallback((work: Promise<unknown>) => void work.catch((e) => onError(String(e))), [onError]);

  // Poll mpv. Only this session's file counts, so a stale reading can't flash the old title.
  useEffect(() => {
    let alive = true;
    const tick = () => {
      const before = { ...sent.current };
      api.state().then(
        (s) => {
          if (!alive) return;
          // Anything sent before this reading was taken is now reflected in it.
          for (const key in before) if (sent.current[key] === before[key]) delete sent.current[key];
          setState(s);
        },
        () => {},
      );
    };
    tick();
    const timer = setInterval(tick, 250);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    setSkipNext(false);
    setCountdown(null);
    setFrame(null);
    setFlash(null);
    setDrag(null);
    frames.current = { busy: false, want: null, last: null };
  }, [mediaFileId]);

  const askFrame = useCallback(
    (seconds: number) => {
      const queue = frames.current;
      const step = Math.round(seconds / FRAME_STEP);
      if (step === queue.last) return;
      queue.last = queue.want = step;
      if (queue.busy) return;
      queue.busy = true;
      const next = () => {
        const wanted = queue.want;
        // A different episode started meanwhile: its own queue takes over.
        if (wanted == null || frames.current !== queue) return void (queue.busy = false);
        queue.want = null;
        api.previewFrame(mediaFileId, wanted * FRAME_STEP).then(
          (src) => frames.current === queue && setFrame({ src, step: wanted }),
          () => {},
        ).finally(next);
      };
      next();
    },
    [mediaFileId],
  );

  const wake = useCallback((soon = false) => {
    setIdle(false);
    clearTimeout(idleTimer.current);
    if (!hold.current) idleTimer.current = window.setTimeout(() => setIdle(true), soon ? AWAY_MS : IDLE_MS);
  }, []);
  const show = useCallback((icon: string, text = "") => setFlash((was) => ({ key: (was?.key ?? 0) + 1, icon, text })), []);
  useEffect(() => {
    if (volumeHeld == null) return;
    const timer = setTimeout(() => setVolumeHeld(null), 600);
    return () => clearTimeout(timer);
  }, [volumeHeld]);
  useEffect(() => {
    if (!drag || drag.held) return;
    const timer = setTimeout(() => setDrag(null), 1200);
    return () => clearTimeout(timer);
  }, [drag]);
  useEffect(() => {
    wake();
    return () => clearTimeout(idleTimer.current);
  }, [wake]);

  // At the end of an episode, count down to the next one.
  const ended = !!state?.ended && state.media_file_id === mediaFileId;
  const offerNext = ended && !!next && !skipNext && !panel;
  useEffect(() => {
    if (!offerNext) return setCountdown(null);
    setCountdown(NEXT_COUNTDOWN);
    const timer = setInterval(() => setCountdown((n) => (n == null || n <= 0 ? n : n - 1)), 1000);
    return () => clearInterval(timer);
  }, [offerNext]);
  useEffect(() => {
    if (countdown !== 0 || !next) return;
    // Clear it first: `next` changes as soon as the new episode loads, and must not fire this again.
    setCountdown(null);
    setSkipNext(true);
    onPlay(next.media_file_id);
  }, [countdown, next, onPlay]);

  // Opening a panel moves focus into it; closing it puts focus on the player itself, so Space pauses again.
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    if (panel === "tracks") el.querySelector<HTMLElement>(".pop button")?.focus();
    else if (panel === "episodes") (el.querySelector<HTMLElement>(".side .row.now") ?? el.querySelector<HTMLElement>(".side .row"))?.focus();
    else if (lastPanel.current) el.focus();
    lastPanel.current = panel;
  }, [panel]);

  const volume = state?.volume ?? 100;
  const speed = state?.speed ?? 1;
  const nudge = useCallback(
    (property: "volume" | "sub-delay" | "sub-scale" | "sub-pos" | "speed", current: number, change: (from: number) => number) => {
      const to = change(sent.current[property] ?? current);
      sent.current[property] = to;
      call(api.setNumber(property, to));
      return to;
    },
    [call],
  );
  const setVolume = useCallback((v: number) => nudge("volume", v, () => Math.max(0, Math.min(130, v))), [nudge]);
  const stepVolume = useCallback((by: number) => nudge("volume", volume, (from) => Math.max(0, Math.min(130, from + by))), [nudge, volume]);
  const toggleMute = useCallback(() => {
    if (volume > 0) {
      lastVolume.current = volume;
      setVolume(0);
      show("muted");
    } else show("volume", `${Math.round(setVolume(lastVolume.current || 100))}%`);
  }, [volume, setVolume, show]);
  const paused = !!state?.paused || ended;
  const pause = useCallback(() => {
    show(paused ? "play" : "pause");
    call(api.togglePause());
  }, [paused, show, call]);
  const fullscreen = useCallback(() => call(api.toggleFullscreen()), [call]);
  const toggle = (which: "tracks" | "episodes") => setPanel((p) => (p === which ? null : which));
  const stepSpeed = useCallback(
    (by: 1 | -1) => {
      const order = [...SPEEDS].sort((a, b) => a - b);
      return nudge("speed", speed, (from) => order[Math.max(0, Math.min(order.length - 1, order.indexOf(from) + by))] ?? 1);
    },
    [nudge, speed],
  );
  const nextAudio = useCallback(() => {
    const audio = state?.tracks.filter((t) => t.kind === "audio") ?? [];
    if (audio.length < 2) return;
    call(api.setTrack("audio", audio[(audio.findIndex((t) => t.selected) + 1) % audio.length].id));
  }, [state, call]);
  const cycleSpeed = () => nudge("speed", speed, (from) => SPEEDS[(SPEEDS.indexOf(from) + 1) % SPEEDS.length]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.closest("input, dialog") || e.ctrlKey || e.altKey || e.metaKey) return;
      wake();
      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      // Space on a focused button presses that button; anywhere else it pauses.
      if (key === " " && target.closest("button")) return;
      const actions: Record<string, () => void> = {
        " ": pause,
        k: pause,
        ArrowLeft: () => (show("rewind", "10 s"), call(api.seek(-10))),
        ArrowRight: () => (show("forward", "10 s"), call(api.seek(10))),
        ArrowUp: () => show("volume", `${Math.round(stepVolume(5))}%`),
        ArrowDown: () => show("volume", `${Math.round(stepVolume(-5))}%`),
        f: fullscreen,
        m: toggleMute,
        n: () => next && onPlay(next.media_file_id),
        s: () => setPanel((p) => (p === "tracks" ? null : "tracks")),
        e: () => series && setPanel((p) => (p === "episodes" ? null : "episodes")),
        i: () => setStats((on) => !on),
        a: nextAudio,
        "[": () => show("speed", `${stepSpeed(-1)}x`),
        "]": () => show("speed", `${stepSpeed(1)}x`),
        Escape: () => (panel ? setPanel(null) : onClose()),
      };
      if (actions[key]) {
        e.preventDefault();
        actions[key]();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [call, wake, pause, show, stepVolume, fullscreen, toggleMute, nextAudio, stepSpeed, next, onPlay, series, panel, onClose]);

  const duration = state?.duration || 0;
  const position = state?.media_file_id === mediaFileId ? state.time_pos : 0;
  const scrubTo = (e: React.PointerEvent<HTMLDivElement>, seek: boolean) => {
    const box = e.currentTarget.getBoundingClientRect();
    const part = Math.min(1, Math.max(0, (e.clientX - box.left) / box.width));
    setPeek({ x: Math.min(box.width - 88, Math.max(88, e.clientX - box.left)), t: part * duration, part, on: true });
    if (duration) askFrame(part * duration);
    if (seek && duration) {
      setDrag({ part, held: true });
      call(api.seek(part * duration, true));
    }
  };
  const volumeTo = (e: React.PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    setVolumeHeld(setVolume(Math.round(Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)) * 100)));
  };
  const shownVolume = volumeHeld ?? volume;
  const volumePart = Math.min(1, shownVolume / 100);
  const release = () => setDrag((d) => d && { ...d, held: false });
  const live = duration ? position / duration : 0;
  // After letting go, stay where the user dropped it until playback reports from there.
  const part = drag && (drag.held || Math.abs(live - drag.part) > 0.004) ? drag.part : live;
  const bar = {
    onPointerEnter: () => {
      hold.current = true;
      wake();
    },
    onPointerLeave: () => {
      hold.current = false;
      wake(away.current);
    },
  };

  const audio = state?.tracks.filter((t) => t.kind === "audio") ?? [];
  const subs = state?.tracks.filter((t) => t.kind === "sub") ?? [];
  const subsOff = !subs.some((t) => t.selected);
  const title = episode?.title ?? movie?.title ?? "";
  const where = episode && series ? [series.title, season && seasonName(season), `episode ${episode.number}`].filter(Boolean).join(", ") : movie?.year ? String(movie.year) : "";
  const hidden = idle && !panel && !state?.paused && !offerNext;

  return (
    <div
      className={`player ${hidden ? "idle" : ""}`}
      ref={root}
      tabIndex={-1}
      onPointerMove={() => {
        away.current = false;
        wake();
      }}
      onPointerDown={() => wake()}
      onWheel={(e) => {
        // The wheel scrolls lists where there are lists; over the video it is the volume.
        if ((e.target as HTMLElement).closest(".pop, .side") || !e.deltaY) return;
        wake();
        show("volume", `${Math.round(stepVolume(e.deltaY < 0 ? 5 : -5))}%`);
      }}
      onPointerLeave={() => {
        away.current = true;
        hold.current = false;
        wake(true);
      }}
    >
      <button type="button" className="stage" aria-label={state?.paused ? "Play" : "Pause"} onClick={() => (panel ? setPanel(null) : pause())} onDoubleClick={fullscreen} />

      {flash && (
        <div className="flash" key={flash.key} aria-hidden="true">
          <Icon name={flash.icon} />
          {flash.text && <span className="num">{flash.text}</span>}
        </div>
      )}

      <div className="top bar" {...bar}>
        <IconButton icon="back" tip="Back (Esc)" onClick={onClose} />
        <div>
          <h3>{title}</h3>
          <div className="small">{where}</div>
        </div>
      </div>

      {stats && state && (
        <div className="stats num" role="status">
          <div>
            {state.width}x{state.height} {state.video_codec}
          </div>
          <div>Decoder: {state.hwdec && state.hwdec !== "no" ? `GPU (${state.hwdec})` : "software"}</div>
          <div>Dropped frames: {state.dropped_frames}</div>
          <div>Speed: {speed}x</div>
        </div>
      )}

      <div className="bottom bar" {...bar}>
        <div
          className={`scrub ${drag?.held ? "drag" : ""}`}
          role="slider"
          tabIndex={0}
          aria-label="Position"
          aria-valuemin={0}
          aria-valuemax={Math.round(duration)}
          aria-valuenow={Math.round(position)}
          aria-valuetext={`${clock(position)} of ${clock(duration)}`}
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            scrubTo(e, true);
          }}
          onPointerMove={(e) => scrubTo(e, e.buttons === 1)}
          onPointerUp={release}
          onLostPointerCapture={release}
          onPointerLeave={() => setPeek((p) => p && { ...p, on: false })}
        >
          <div className="track">
            <div className="ghost" style={{ transform: `scaleX(${peek?.on ? peek.part : 0})` }} />
            <div className="fill" style={{ transform: `scaleX(${part})` }} />
          </div>
          <div className="knob" style={{ transform: `translateX(${part * 100}%)` }}>
            <i />
          </div>
          {peek && duration > 0 && (
            <span className={`peek ${peek.on ? "on" : ""}`} style={{ transform: `translateX(${peek.x}px) translateX(-50%)` }}>
              <span className="peek-box num">
                {/* Only a frame from near the pointer: an old one would mislead. */}
                {frame && Math.abs(frame.step * FRAME_STEP - peek.t) <= FRAME_STEP * 3 && <img src={frame.src} alt="" />}
                <span>{clock(peek.t)}</span>
              </span>
            </span>
          )}
        </div>
        <div className="ctl">
          <IconButton icon={paused ? "play" : "pause"} tip={paused ? "Play (Space)" : "Pause (Space)"} onClick={() => call(api.togglePause())} />
          <IconButton icon="rewind" tip="Back 10 seconds (Left)" onClick={() => call(api.seek(-10))} />
          <IconButton icon="forward" tip="Forward 10 seconds (Right)" onClick={() => call(api.seek(10))} />
          <IconButton icon={shownVolume > 0 ? "volume" : "muted"} tip={volume > 0 ? "Mute (M)" : "Unmute (M)"} onClick={toggleMute} />
          <div
            className={`volume ${volumeHeld != null ? "drag" : ""}`}
            role="slider"
            tabIndex={0}
            aria-label="Volume"
            aria-valuemin={0}
            aria-valuemax={130}
            aria-valuenow={Math.round(shownVolume)}
            aria-valuetext={`${Math.round(shownVolume)}%`}
            data-tip="Volume (Up, Down or the mouse wheel)"
            onPointerDown={(e) => {
              e.currentTarget.setPointerCapture(e.pointerId);
              volumeTo(e);
            }}
            onPointerMove={(e) => e.buttons === 1 && volumeTo(e)}
          >
            <div className="track">
              <div className="fill" style={{ transform: `scaleX(${volumePart})` }} />
            </div>
            <div className="knob" style={{ transform: `translateX(${volumePart * 100}%)` }}>
              <i />
            </div>
          </div>
          <span className="small num vol">{Math.round(shownVolume)}%</span>
          <span className="time num">
            {clock(position)} / {clock(duration)}
          </span>
          <span className="gap" />
          {next && <IconButton icon="next" tip="Next episode (N)" onClick={() => onPlay(next.media_file_id)} />}
          {series && <IconButton icon="list" tip="Episodes (E)" onClick={() => toggle("episodes")} />}
          <IconButton icon="subs" tip="Audio and subtitles (S)" onClick={() => toggle("tracks")} />
          <IconButton icon="speed" tip={`Change speed (${speed}x)`} onClick={cycleSpeed} />
          <IconButton icon="fullscreen" tip="Fullscreen (F)" onClick={fullscreen} />
        </div>
      </div>

      <div className={`pop ${panel === "tracks" ? "on" : ""}`} inert={panel !== "tracks"} role="group" aria-label="Audio and subtitles">
        <h4 id="pop-audio">Audio</h4>
        <div role="radiogroup" aria-labelledby="pop-audio">
          {audio.map((t, i) => (
            <button type="button" key={t.id} role="radio" aria-checked={t.selected} onClick={() => call(api.setTrack("audio", t.id))}>
              <span>{trackName(t, i)}</span>
              <span className="meta">{trackMeta(t)}</span>
            </button>
          ))}
        </div>
        {!audio.length && <p className="small">This file has no audio.</p>}
        <h4 id="pop-subs">Subtitles</h4>
        <div role="radiogroup" aria-labelledby="pop-subs">
          <button type="button" role="radio" aria-checked={subsOff} onClick={() => call(api.setTrack("sub", null))}>
            <span>Off</span>
          </button>
          {subs.map((t, i) => (
            <button type="button" key={t.id} role="radio" aria-checked={t.selected} onClick={() => call(api.setTrack("sub", t.id))}>
              <span>{trackName(t, i)}</span>
              <span className="meta">{trackMeta(t)}</span>
            </button>
          ))}
        </div>
        <button type="button" onClick={() => call(pickSubtitle().then((path) => (path ? api.addSubtitle(path) : undefined)))}>
          <span>Add subtitle file</span>
          <Icon name="plus" />
        </button>
        <h4>Subtitle timing, size and position</h4>
        <Stepper label="Delay" value={`${(state?.sub_delay ?? 0).toFixed(1)} s`} onStep={(d) => nudge("sub-delay", state?.sub_delay ?? 0, (from) => Math.round((from + d * 0.1) * 10) / 10)} />
        <Stepper label="Size" value={`${Math.round((state?.sub_scale ?? 1) * 100)}%`} onStep={(d) => nudge("sub-scale", state?.sub_scale ?? 1, (from) => Math.max(0.5, Math.min(3, Math.round((from + d * 0.1) * 10) / 10)))} />
        <Stepper
          label="Raised"
          value={`${Math.round(100 - (state?.sub_pos ?? 100))}%`}
          less="Move subtitles down"
          more="Move subtitles up"
          onStep={(d) => nudge("sub-pos", state?.sub_pos ?? 100, (from) => Math.max(0, Math.min(100, from - d * 5)))}
        />
      </div>

      {next && (
        <div className={`next ${offerNext ? "on" : ""}`} inert={!offerNext}>
          <Thumb src={thumb(lib, next)} />
          <div className="txt">
            <div>
              <div className="meta num">Next episode in {countdown ?? NEXT_COUNTDOWN} s</div>
              <div className="title">{next.title}</div>
            </div>
            <div className="actions">
              <button type="button" className="link" onClick={() => onPlay(next.media_file_id)}>
                Play now
              </button>
              <button type="button" className="link" onClick={() => setSkipNext(true)}>
                Stay here
              </button>
            </div>
          </div>
        </div>
      )}

      {series && season && (
        <aside className={`side ${panel === "episodes" ? "on" : ""}`} inert={panel !== "episodes"} aria-label="Episodes">
          <header>
            <h3>{seasonName(season)}</h3>
            <IconButton icon="close" tip="Close episodes (E)" onClick={() => setPanel(null)} />
          </header>
          <div className="list">
            {season.episodes.map((e) => (
              <button type="button" className={`row ${e.media_file_id === mediaFileId ? "now" : ""}`} key={e.id} aria-current={e.media_file_id === mediaFileId} onClick={() => onPlay(e.media_file_id)}>
                <span className="n num">{e.number}</span>
                <Thumb src={thumb(lib, e)} />
                <span className="text">
                  <span className="title">{e.title}</span>
                  <span className="small num">{[runtime(e), e.watched && "watched"].filter(Boolean).join(", ")}</span>
                </span>
              </button>
            ))}
          </div>
        </aside>
      )}
    </div>
  );
}

function Stepper({ label, value, less, more, onStep }: { label: string; value: string; less?: string; more?: string; onStep: (direction: 1 | -1) => void }) {
  return (
    <div className="stepper">
      <span>{label}</span>
      <IconButton icon="minus" tip={less ?? `Reduce ${label.toLowerCase()}`} onClick={() => onStep(-1)} />
      <span className="meta num value">{value}</span>
      <IconButton icon="plus" tip={more ?? `Increase ${label.toLowerCase()}`} onClick={() => onStep(1)} />
    </div>
  );
}
