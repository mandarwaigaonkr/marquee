import { useLayoutEffect, useRef, useState } from "react";
import {
  type Episode,
  type Library,
  type Series,
  api,
  code,
  episodesOf,
  fraction,
  placement,
  placements,
  playLabel,
  plural,
  resumeEpisode,
  runtime,
  seasonName,
  thumb,
  watchedCount,
} from "../api";
import { Confirm, Frame, Icon, InlineEdit, Menu, type MenuItem, Progress, RowMain, Thumb } from "./ui";

type Props = {
  lib: Library;
  series: Series;
  onBack: () => void;
  onPlay: (mediaFileId: number) => void;
  /** Add files straight into this series and season. */
  onAddFiles: (series: Series, seasonNumber: number) => void;
  /** Point an episode at a file picked by hand. */
  onLocate: (mediaFileId: number) => void;
  onFindMoved: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
};

type Editing = { kind: "series" | "season" | "episode"; id: number } | null;
type Removing = { kind: "series"; name: string } | { kind: "episode"; name: string; id: number } | null;

export function SeriesPage({ lib, series, onBack, onPlay, onAddFiles, onLocate, onFindMoved, onChanged, onError }: Props) {
  const resume = resumeEpisode(series);
  const [picked, setPicked] = useState<number | null>(null);
  const [editing, setEditing] = useState<Editing>(null);
  const [removing, setRemoving] = useState<Removing>(null);
  const [placing, setPlacing] = useState<Episode | null>(null);
  const season = series.seasons.find((s) => s.number === (picked ?? resume?.season_number)) ?? series.seasons[0];
  const all = episodesOf(series);
  const frame = thumb(lib, resume) ?? thumb(lib, all.find((e) => e.has_thumb));
  const run = (work: Promise<unknown>) => work.then(onChanged, (e) => onError(String(e)));
  const is = (kind: NonNullable<Editing>["kind"], id: number) => editing?.kind === kind && editing.id === id;
  const rename = (kind: NonNullable<Editing>["kind"], id: number) => (next: string | null) => {
    setEditing(null);
    if (next) run(api.rename(kind, id, next));
  };

  const seriesMenu: MenuItem[] = [
    { label: "Rename series", onSelect: () => setEditing({ kind: "series", id: series.id }) },
    ...(season ? [{ label: `Rename ${seasonName(season)}`, onSelect: () => setEditing({ kind: "season", id: season.id }) }] : []),
    { label: "Add files to this season", onSelect: () => onAddFiles(series, season?.number ?? 1) },
    { label: "Add a season", onSelect: () => run(api.addSeason(series.id).then(setPicked)) },
    { label: "Find moved files", onSelect: onFindMoved },
    { label: "Remove series from library", danger: true, onSelect: () => setRemoving({ kind: "series", name: series.title }) },
  ];

  const episodeMenu = (e: Episode): MenuItem[] => [
    { label: "Rename", onSelect: () => setEditing({ kind: "episode", id: e.id }) },
    {
      label: "Move to season",
      items: [
        ...series.seasons.filter((s) => s.number !== e.season_number).map((s) => ({ label: seasonName(s), onSelect: () => run(api.moveEpisode(e.id, s.number)) })),
        // Specials is always a destination, even before the series has any.
        ...(e.season_number !== 0 && !series.seasons.some((s) => s.number === 0) ? [{ label: "Specials", onSelect: () => run(api.moveEpisode(e.id, 0)) }] : []),
        { label: "New season", onSelect: () => run(api.addSeason(series.id).then((n) => api.moveEpisode(e.id, n))) },
      ],
    },
    { label: e.season_number === 0 ? "Change number or where it plays" : "Change number", onSelect: () => setPlacing(e) },
    { label: e.watched ? "Mark as unwatched" : "Mark as watched", onSelect: () => run(api.setWatched(e.media_file_id, !e.watched)) },
    { label: "Locate file", onSelect: () => onLocate(e.media_file_id) },
    { label: "Remove from library", danger: true, onSelect: () => setRemoving({ kind: "episode", name: e.title, id: e.id }) },
  ];

  return (
    <>
      <div className="series-top">
        <div className="info">
          <button type="button" className="back" onClick={onBack}>
            <Icon name="back" />
            Home
          </button>
          <h1>
            <InlineEdit value={series.title} editing={is("series", series.id)} label="Series name" onDone={rename("series", series.id)} />
          </h1>
          <p className="small">
            {plural(series.seasons.length, "season")}, {plural(all.length, "episode")}, {watchedCount(series)} watched
          </p>
          <div className="actions">
            <button type="button" className="btn primary" disabled={!resume} onClick={() => resume && onPlay(resume.media_file_id)}>
              <Icon name="play" />
              {playLabel(series)}
            </button>
            <Menu trigger="Edit series" label="Edit series" items={seriesMenu} className="left" />
          </div>
        </div>
        <div className="shot">{frame && <Frame src={frame} />}</div>
      </div>

      <div className="page">
        <div className="tabs" role="tablist" aria-label="Seasons">
          {series.seasons.map((s) =>
            is("season", s.id) ? (
              <div className="tab" key={s.id}>
                <InlineEdit value={seasonName(s)} editing fit label="Season name" onDone={rename("season", s.id)} />
              </div>
            ) : (
              <button
                type="button"
                role="tab"
                className="tab"
                key={s.id}
                aria-selected={s === season}
                aria-label={`${seasonName(s)}, ${s.episodes.filter((e) => e.watched).length} of ${plural(s.episodes.length, "episode")} watched`}
                onClick={() => setPicked(s.number)}
              >
                {seasonName(s)}
                {/* One mark per episode: the whole series' progress at a glance. */}
                <span className="map" aria-hidden="true">
                  {s.episodes.map((e) => (
                    <i key={e.id} className={e.watched ? "done" : e.position_ms > 0 ? "part" : ""} />
                  ))}
                </span>
              </button>
            ),
          )}
        </div>
        {season && season.episodes.length > 0 ? (
          <div className="eps" key={season.id}>
            {season.episodes.map((e) => {
              const part = !e.watched && e.position_ms > 0;
              return (
                <div className={`row with-menu ${e.id === resume?.id ? "now" : ""}`} key={e.id}>
                  <RowMain editing={is("episode", e.id)} label={`Play episode ${e.number}, ${e.title}`} onClick={() => onPlay(e.media_file_id)}>
                    <span className="n num">{e.number}</span>
                    <Thumb src={thumb(lib, e)} />
                    <span className="text">
                      <span className="title">
                        <InlineEdit value={e.title} editing={is("episode", e.id)} label="Episode name" onDone={rename("episode", e.id)} />
                      </span>
                      <span className="small num">{[placement(series, e), runtime(e), e.watched && "watched"].filter(Boolean).join(", ") || " "}</span>
                    </span>
                    <span className="end">
                      {part && <Progress value={fraction(e)} />}
                      {e.watched && <Icon name="check" />}
                    </span>
                  </RowMain>
                  <Menu trigger={<Icon name="more" />} label={`More actions for episode ${e.number}`} items={episodeMenu(e)} />
                </div>
              );
            })}
          </div>
        ) : (
          <div className="eps none">
            <p>{season ? `${seasonName(season)} has no episodes yet.` : "This series has no episodes yet."}</p>
            <button type="button" className="btn quiet" onClick={() => onAddFiles(series, season?.number ?? 1)}>
              <Icon name="plus" />
              Add files
            </button>
          </div>
        )}
      </div>

      {placing && (
        <PlaceEpisode
          series={series}
          episode={placing}
          onCancel={() => setPlacing(null)}
          onSave={(work) => {
            setPlacing(null);
            run(work);
          }}
        />
      )}

      {removing && (
        <Confirm
          title={`Remove ${removing.name} from the library?`}
          body={
            removing.kind === "series"
              ? `This removes ${removing.name} and its ${plural(all.length, "episode")} from Marquee, along with their watch progress. The video files stay on your disk.`
              : "Its watch progress is forgotten. The video file stays on your disk."
          }
          action={`Remove ${removing.name}`}
          onCancel={() => setRemoving(null)}
          onConfirm={() => {
            setRemoving(null);
            if (removing.kind === "series") api.remove("series", series.id).then(() => (onBack(), onChanged()), (e) => onError(String(e)));
            else run(api.remove("episode", removing.id));
          }}
        />
      )}
    </>
  );
}

/**
 * One episode's season, number and, for a special, where it plays in watch order.
 * Native dialog, so focus is trapped and handed back.
 */
function PlaceEpisode({ series, episode, onCancel, onSave }: { series: Series; episode: Episode; onCancel: () => void; onSave: (work: Promise<unknown>) => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [season, setSeason] = useState(episode.season_number);
  const [number, setNumber] = useState(String(episode.number));
  const [invalid, setInvalid] = useState(false);
  // Every spot but the episode itself: it can't play after itself.
  const spots = placements(series).filter((p) => !(p.season === episode.season_number && p.number === episode.number && episode.season_number !== 0));
  const current = () => {
    // A special already placed: the spot it resolves to. One moving in from a season: its old spot.
    const [s, n] = episode.season_number === 0 ? [episode.after_season, episode.after_number] : [episode.season_number, episode.number - 1];
    const last = (match: (p: (typeof spots)[number]) => boolean) => spots.reduce((found, p, i) => (match(p) ? i : found), -1);
    if (s == null) return spots.length - 1;
    const at = n == null ? last((p) => p.season === s) : last((p) => p.season === s && p.number != null && p.number <= n);
    return at >= 0 ? at : spots.length - 1;
  };
  const [spot, setSpot] = useState(current);
  const seasons = [...series.seasons.map((s) => ({ number: s.number, name: seasonName(s) })), ...(series.seasons.some((s) => s.number === 0) ? [] : [{ number: 0, name: "Specials" }])];

  useLayoutEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    // showModal focuses the first field, which is the right one here.
    return () => dialog?.close();
  }, []);

  const save = () => {
    const n = Number(number);
    if (!/^\d+$/.test(number.trim()) || !Number.isSafeInteger(n)) return setInvalid(true);
    const moved = season !== episode.season_number || n !== episode.number;
    const chosen = spots[spot];
    const placed = season === 0 && (moved || chosen.season !== episode.after_season || chosen.number !== episode.after_number);
    onSave(
      (moved ? api.moveEpisode(episode.id, season, n) : Promise.resolve()).then(() => (placed ? api.placeSpecial(episode.id, chosen.season, chosen.number) : undefined)),
    );
  };

  return (
    <dialog ref={ref} className="confirm" onCancel={onCancel} onClick={(e) => e.target === ref.current && onCancel()}>
      <form
        className="confirm-body"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2>Change {code(episode)}</h2>
        <div className="place-fields">
          <label>
            <span className="meta">Season</span>
            <select className="field" value={season} onChange={(e) => setSeason(Number(e.target.value))}>
              {seasons.map((s) => (
                <option key={s.number} value={s.number}>
                  {s.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span className="meta">Number</span>
            <input
              className="field num"
              inputMode="numeric"
              value={number}
              aria-invalid={invalid}
              aria-describedby={invalid ? "place-number-error" : undefined}
              onChange={(e) => (setInvalid(false), setNumber(e.target.value))}
              onBlur={() => setInvalid(!/^\d+$/.test(number.trim()))}
            />
          </label>
          {invalid && (
            <span className="error wide" id="place-number-error">
              Type a whole number, 0 or more.
            </span>
          )}
          {season === 0 && (
            <label className="wide">
              <span className="meta">Plays</span>
              <select className="field" value={spot} onChange={(e) => setSpot(Number(e.target.value))}>
                {spots.map((p, i) => (
                  <option key={i} value={i}>
                    {p.label}
                  </option>
                ))}
              </select>
              <span className="small">Play and the next-episode countdown follow this order.</span>
            </label>
          )}
        </div>
        <div className="actions">
          <button type="button" className="btn quiet" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className="btn primary">
            {/^\d+$/.test(number.trim()) ? `Save ${code({ ...episode, season_number: season, number: Number(number) })}` : "Save changes"}
          </button>
        </div>
      </form>
    </dialog>
  );
}
