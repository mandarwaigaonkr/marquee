import { useState } from "react";
import { type Episode, type Library, type Series, api, episodesOf, fraction, playLabel, plural, resumeEpisode, runtime, seasonName, thumb, watchedCount } from "../api";
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
        { label: "New season", onSelect: () => run(api.addSeason(series.id).then((n) => api.moveEpisode(e.id, n))) },
      ],
    },
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
                      <span className="small num">{[runtime(e), e.watched && "watched"].filter(Boolean).join(", ") || " "}</span>
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
