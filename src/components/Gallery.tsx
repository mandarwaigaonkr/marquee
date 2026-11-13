import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { type Library, type Series, episodesOf, playLabel, plural, resumeEpisode, thumb, watchedCount } from "../api";
import { Frame, Icon } from "./ui";

const MIN_OPEN = 480;

/**
 * The expanding gallery (the only one in the app). Every panel is laid out at
 * its open width and only ever moves with a transform, so opening one never
 * triggers layout or repaint.
 */
export function Gallery({ lib, onOpen, onPlay, onAdd }: { lib: Library; onOpen: (s: Series) => void; onPlay: (s: Series) => void; onAdd: () => void }) {
  const root = useRef<HTMLDivElement>(null);
  const hover = useRef<number>(undefined);
  const focusCaption = useRef(false);
  const [width, setWidth] = useState(0);
  const [openIdx, setOpenIdx] = useState(0);
  const [ready, setReady] = useState(false);
  const count = lib.series.length;
  const at = Math.min(openIdx, count - 1);

  useLayoutEffect(() => {
    const el = root.current!;
    const measure = () => setWidth(el.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  // Transitions switch on only after the first layout, so panels don't slide in from zero.
  useEffect(() => {
    if (width && !ready) requestAnimationFrame(() => setReady(true));
  }, [width, ready]);
  useEffect(() => () => clearTimeout(hover.current), []);
  // A spine fades out when its panel opens, so keyboard focus moves on to that panel's Play button.
  useEffect(() => {
    if (!focusCaption.current) return;
    focusCaption.current = false;
    root.current?.querySelector<HTMLElement>(".panel.on .cap button:not(:disabled)")?.focus();
  }, [openIdx]);

  const closed = width < 720 ? 56 : 96;
  const open = Math.max(Math.min(MIN_OPEN, width - closed), width - closed * count);
  const x = (i: number) => i * closed + (i > at ? open - closed : 0);
  const vars = { "--closed": `${closed}px`, "--open": `${open}px`, "--drift": `${-(open - closed) / 2}px` } as React.CSSProperties;

  // Skip panels the pointer only crosses on its way somewhere else.
  const intent = (i: number) => {
    clearTimeout(hover.current);
    hover.current = window.setTimeout(() => setOpenIdx(i), 70);
  };

  return (
    <div className={`gallery ${ready ? "ready" : ""}`} ref={root} style={vars} onPointerLeave={() => clearTimeout(hover.current)}>
      <div className="gallery-track" style={{ width: closed * count + open }}>
        {lib.series.map((s, i) => {
          const on = i === at;
          const total = episodesOf(s).length;
          const frame = thumb(lib, resumeEpisode(s)) ?? thumb(lib, episodesOf(s).find((e) => e.has_thumb));
          return (
            <div key={s.id} className={`panel ${on ? "on" : ""}`} style={{ transform: `translateX(${x(i)}px)` }} onPointerEnter={() => intent(i)} onClick={() => onOpen(s)}>
              <div className="shot">{frame && <Frame src={frame} />}</div>
              <div className="dim" />
              <button type="button" className="spine" aria-label={`Open ${s.title}`} tabIndex={on ? -1 : 0} onFocus={() => ((focusCaption.current = true), setOpenIdx(i))}>
                <span>{s.title}</span>
              </button>
              <div className="count num">
                {watchedCount(s)}/{total}
              </div>
              <div className="cap" inert={!on}>
                <div className="cap-text">
                  <h3>{s.title}</h3>
                  <div className="small">
                    {plural(s.seasons.length, "season")}, {watchedCount(s)} of {total} watched
                  </div>
                </div>
                <div className="actions">
                  <button
                    type="button"
                    className="btn primary"
                    disabled={!total}
                    onClick={(e) => {
                      e.stopPropagation();
                      onPlay(s);
                    }}
                  >
                    <Icon name="play" />
                    {playLabel(s)}
                  </button>
                  <button
                    type="button"
                    className="btn quiet"
                    onClick={(e) => {
                      e.stopPropagation();
                      onOpen(s);
                    }}
                  >
                    Open series
                  </button>
                </div>
              </div>
            </div>
          );
        })}
        <button type="button" className="panel add" style={{ transform: `translateX(${(count - 1) * closed + open}px)` }} onClick={onAdd}>
          <Icon name="plus" />
          <span className="meta">Add files</span>
        </button>
      </div>
    </div>
  );
}
