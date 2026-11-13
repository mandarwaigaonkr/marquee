import { useState } from "react";
import { type Episode, type Library, type Movie, type Series, api, code, episodesOf, fraction, plural, resumeEpisode, runtime, thumb } from "../api";
import { Gallery } from "./Gallery";
import { Confirm, Frame, Icon, InlineEdit, Menu, Progress, RowMain, Thumb } from "./ui";

type Props = {
  lib: Library;
  onOpen: (s: Series) => void;
  onPlay: (mediaFileId: number) => void;
  onAdd: (what: "files" | "folder") => void;
  onSearch: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
};

type Ongoing = { key: string; at: number; title: string; where: string; item: Episode | Movie; series?: Series };

/** What is half-watched, most recent first: one entry per series, plus movies. */
function ongoing(lib: Library): Ongoing[] {
  const out: Ongoing[] = [];
  for (const s of lib.series) {
    const all = episodesOf(s);
    const e = resumeEpisode(s);
    const touched = all.filter((x) => x.last_played_at != null);
    if (!e || !touched.length || all.every((x) => x.watched)) continue;
    out.push({ key: `s${s.id}`, at: Math.max(...touched.map((x) => x.last_played_at!)), title: e.title, where: `${s.title}, season ${e.season_number} episode ${e.number}`, item: e, series: s });
  }
  for (const m of lib.movies) {
    if (m.position_ms > 0 && !m.watched) out.push({ key: `m${m.id}`, at: m.last_played_at ?? 0, title: m.title, where: m.year ? `Movie, ${m.year}` : "Movie", item: m });
  }
  return out.sort((a, b) => b.at - a.at);
}

export function Home({ lib, onOpen, onPlay, onAdd, onSearch, onChanged, onError }: Props) {
  const [renaming, setRenaming] = useState<number | null>(null);
  const [removing, setRemoving] = useState<Movie | null>(null);
  const list = ongoing(lib);
  const [first, ...rest] = list;
  const episodes = lib.series.reduce((n, s) => n + episodesOf(s).length, 0);
  const run = (work: Promise<unknown>) => work.then(onChanged, (e) => onError(String(e)));

  const jump = (
    <button type="button" className="jump" onClick={onSearch}>
      <Icon name="search" />
      <span>Jump to anything</span>
      <kbd>Ctrl K</kbd>
    </button>
  );

  return (
    <div className="page">
      {first && (
        <section>
          <div className="head">
            <h2>Continue watching</h2>
            {jump}
          </div>
          <div className="continue">
            <div className="shot">{first.item.has_thumb && <Frame src={thumb(lib, first.item)!} />}</div>
            <div className="info">
              <div className="small">{first.where}</div>
              <h1>{first.title}</h1>
              <Progress value={fraction(first.item)} />
              <div className="small num">{runtime(first.item)}</div>
              <div className="actions">
                <button type="button" className="btn primary" onClick={() => onPlay(first.item.media_file_id)}>
                  <Icon name="play" />
                  {first.item.position_ms > 0 ? "Resume" : "Play"}
                </button>
                {first.series && (
                  <button type="button" className="btn quiet" onClick={() => onOpen(first.series!)}>
                    Open series
                  </button>
                )}
              </div>
            </div>
          </div>
          {rest.length > 0 && (
            <div className="rows">
              {rest.map((o) => (
                <button type="button" className="row" key={o.key} onClick={() => onPlay(o.item.media_file_id)}>
                  <Thumb src={thumb(lib, o.item)} />
                  <span className="text">
                    <span className="title">{o.series ? o.series.title : o.title}</span>
                    <span className="small">{o.series ? `${code(o.item as Episode)}: ${o.title}` : o.where}</span>
                  </span>
                  <span className="end">
                    <Progress value={fraction(o.item)} />
                    <span className="num">{runtime(o.item)}</span>
                  </span>
                </button>
              ))}
            </div>
          )}
        </section>
      )}

      {lib.series.length > 0 && (
        <section>
          <div className="head">
            <h2>Library</h2>
            {first ? (
              <span className="small">
                {lib.series.length} series, {plural(episodes, "episode")}. Drop files anywhere to add them.
              </span>
            ) : (
              jump
            )}
          </div>
          <Gallery lib={lib} onOpen={onOpen} onPlay={(s) => resumeEpisode(s) && onPlay(resumeEpisode(s)!.media_file_id)} onAdd={() => onAdd("files")} />
        </section>
      )}

      {lib.movies.length > 0 && (
        <section>
          <div className="head">
            <h2>Movies</h2>
            {!first && !lib.series.length ? jump : <span className="small">{plural(lib.movies.length, "movie")}</span>}
          </div>
          <div className="rows framed">
            {lib.movies.map((m) => (
              <div className="row with-menu" key={m.id}>
                <RowMain editing={renaming === m.id} label={`Play ${m.title}`} onClick={() => onPlay(m.media_file_id)}>
                  <Thumb src={thumb(lib, m)} />
                  <span className="text">
                    <span className="title">
                      <InlineEdit value={m.title} editing={renaming === m.id} label="Movie name" onDone={(next) => (setRenaming(null), next && run(api.rename("movie", m.id, next)))} />
                    </span>
                    <span className="small num">{[m.year, runtime(m), m.watched && "watched"].filter(Boolean).join(", ")}</span>
                  </span>
                  <span className="end">{!m.watched && m.position_ms > 0 && <Progress value={fraction(m)} />}</span>
                </RowMain>
                <Menu
                  trigger={<Icon name="more" />}
                  label={`More actions for ${m.title}`}
                  items={[
                    { label: "Rename", onSelect: () => setRenaming(m.id) },
                    { label: m.watched ? "Mark as unwatched" : "Mark as watched", onSelect: () => run(api.setWatched(m.media_file_id, !m.watched)) },
                    { label: "Remove from library", danger: true, onSelect: () => setRemoving(m) },
                  ]}
                />
              </div>
            ))}
          </div>
        </section>
      )}

      {removing && (
        <Confirm
          title={`Remove ${removing.title} from the library?`}
          body="Its watch progress is forgotten. The video file stays on your disk."
          action={`Remove ${removing.title}`}
          onCancel={() => setRemoving(null)}
          onConfirm={() => (setRemoving(null), run(api.remove("movie", removing.id)))}
        />
      )}
    </div>
  );
}

export function EmptyLibrary({ onAdd }: { onAdd: (what: "files" | "folder") => void }) {
  return (
    <div className="empty">
      <h1>Add your videos</h1>
      <p>Pick a folder, or drop files anywhere on this window. Marquee reads names like S02E01 and sorts them into series and seasons. Your files stay where they are.</p>
      <div className="actions">
        <button type="button" className="btn primary" onClick={() => onAdd("folder")}>
          <Icon name="plus" />
          Add folder
        </button>
        <button type="button" className="btn quiet" onClick={() => onAdd("files")}>
          Add files
        </button>
      </div>
    </div>
  );
}
