import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type Playable = {
  id: number;
  title: string;
  media_file_id: number;
  path: string;
  has_thumb: boolean;
  duration_ms: number;
  position_ms: number;
  watched: boolean;
  last_played_at: number | null;
};
/** `after_season`/`after_number`: where a special plays (db.rs SCHEMA_V3). Number 0 is before the season, null after it. */
export type Episode = Playable & { season_number: number; number: number; number_end: number | null; after_season: number | null; after_number: number | null };
export type Movie = Playable & { year: number | null };
export type Season = { id: number; number: number; title: string | null; episodes: Episode[] };
export type Series = { id: number; title: string; seasons: Season[]; resume_episode_id: number | null; watch_order: number[] };
export type Library = { series: Series[]; movies: Movie[]; thumbs_dir: string };

export type Proposed = {
  path: string;
  kind: "episode" | "movie" | "unsorted";
  series: string;
  season: number;
  season_title: string | null;
  episode: number;
  episode_end: number | null;
  title: string;
  year: number | null;
  include: boolean;
  after_season: number | null;
};

export type Track = { id: number; kind: "audio" | "sub"; lang: string; title: string; codec: string; selected: boolean; external: boolean };
export type PlayerState = {
  media_file_id: number | null;
  time_pos: number;
  duration: number;
  paused: boolean;
  idle: boolean;
  ended: boolean;
  volume: number;
  sub_delay: number;
  sub_scale: number;
  sub_pos: number;
  speed: number;
  hwdec: string;
  video_codec: string;
  width: number;
  height: number;
  dropped_frames: number;
  tracks: Track[];
};

export type WatchFolder = { id: number; path: string };
export type ItemKind = "series" | "season" | "episode" | "movie";

export const api = {
  library: () => invoke<Library>("library"),
  importScan: (paths: string[]) => invoke<Proposed[]>("import_scan", { paths }),
  importCommit: (items: Proposed[]) => invoke<number>("import_commit", { items }),
  rename: (kind: ItemKind, id: number, title: string) => invoke<void>("rename_item", { kind, id, title }),
  remove: (kind: ItemKind, id: number) => invoke<void>("remove_item", { kind, id }),
  createSeries: (title: string) => invoke<number>("create_series", { title }),
  addSeason: (seriesId: number) => invoke<number>("add_season", { seriesId }),
  moveEpisode: (episodeId: number, seasonNumber: number, number: number | null = null) => invoke<void>("move_episode", { episodeId, seasonNumber, number }),
  /** Where a special plays: after episode `afterNumber` of `afterSeason` (0 before it, null after it); no season = at the end. */
  placeSpecial: (episodeId: number, afterSeason: number | null, afterNumber: number | null) => invoke<void>("place_special", { episodeId, afterSeason, afterNumber }),
  setWatched: (mediaFileId: number, watched: boolean) => invoke<void>("set_watched", { mediaFileId, watched }),
  play: (mediaFileId: number, fromStart = false) => invoke<boolean>("player_play", { mediaFileId, fromStart }),
  stop: () => invoke<void>("player_stop"),
  togglePause: () => invoke<void>("player_toggle_pause"),
  seek: (seconds: number, absolute = false) => invoke<void>("player_seek", { seconds, absolute }),
  setTrack: (kind: "audio" | "sub", id: number | null) => invoke<void>("player_set_track", { kind, id }),
  fileMissing: (mediaFileId: number) => invoke<boolean>("file_missing", { mediaFileId }),
  relinkFile: (mediaFileId: number, path: string) => invoke<void>("relink_file", { mediaFileId, path }),
  /** [found again, still missing] */
  relinkFolder: (paths: string[]) => invoke<[number, number]>("relink_folder", { paths }),
  watchFolders: () => invoke<WatchFolder[]>("watch_folders"),
  addWatchFolder: (path: string) => invoke<number>("add_watch_folder", { path }),
  removeWatchFolder: (id: number) => invoke<void>("remove_watch_folder", { id }),
  setNumber: (property: "volume" | "sub-delay" | "sub-scale" | "sub-pos" | "speed", value: number) => invoke<void>("player_set_number", { property, value }),
  addSubtitle: (path: string) => invoke<void>("player_add_subtitle", { path }),
  state: () => invoke<PlayerState>("player_state"),
  /** Image address of a small frame near `seconds`, for the scrubber. Rejects when none can be made. */
  previewFrame: (mediaFileId: number, seconds: number) => invoke<string>("preview_frame", { mediaFileId, seconds }).then(convertFileSrc),
  toggleFullscreen: () => invoke<boolean>("toggle_fullscreen"),
};

// Must list exactly what the organiser accepts (VIDEO_EXTENSIONS in organiser.rs).
const VIDEO = ["mkv", "mp4", "avi", "mov", "webm", "m4v", "ts", "m2ts", "wmv", "flv", "mpg", "mpeg", "ogm"];
export const SUBS = ["srt", "ass", "ssa", "vtt", "sub", "idx", "sup"];

const list = (picked: string | string[] | null) => (picked == null ? [] : Array.isArray(picked) ? picked : [picked]);
export const pickFolders = async (title = "Add folder") => list(await open({ directory: true, multiple: true, title }));
export const pickFiles = async () => list(await open({ multiple: true, title: "Add files", filters: [{ name: "Video", extensions: VIDEO }] }));
export const pickFolder = async (title: string) => list(await open({ directory: true, title }))[0];
export const pickFile = async (title: string) => list(await open({ title, filters: [{ name: "Video", extensions: VIDEO }] }))[0];
export const pickSubtitle = async () => list(await open({ title: "Add subtitle file", filters: [{ name: "Subtitles", extensions: SUBS }] }))[0];

export const thumb = (lib: Library, item: { media_file_id: number; has_thumb: boolean } | undefined) =>
  item?.has_thumb ? convertFileSrc(`${lib.thumbs_dir}\\${item.media_file_id}.jpg`) : undefined;

// ---- derived library facts ----

export const episodesOf = (s: Series) => s.seasons.flatMap((season) => season.episodes);
export const resumeEpisode = (s: Series) => episodesOf(s).find((e) => e.id === s.resume_episode_id);
/** The episode or film a media file belongs to. */
export const itemOf = (lib: Library, mediaFileId: number): Playable | undefined =>
  lib.series.flatMap(episodesOf).find((e) => e.media_file_id === mediaFileId) ?? lib.movies.find((m) => m.media_file_id === mediaFileId);
export const watchedCount = (s: Series) => episodesOf(s).filter((e) => e.watched).length;
export const seasonName = (season: Season) => season.title || (season.number === 0 ? "Specials" : `Season ${season.number}`);
export const code = (e: Episode) => (e.season_number === 0 ? `Special ${e.number}` : `S${e.season_number} E${e.number}${e.number_end ? `-${e.number_end}` : ""}`);
/** "season 2 episode 5", or "special 1". */
export const spoken = (e: Episode) => (e.season_number === 0 ? `special ${e.number}` : `season ${e.season_number} episode ${e.number}`);
/** Episodes in the order they are watched: placed specials sit between episodes, the rest come last. */
export function watchOrder(s: Series) {
  const byId = new Map(episodesOf(s).map((e) => [e.id, e]));
  return s.watch_order.flatMap((id) => byId.get(id) ?? []);
}

/** Every spot a special can play in a series, in watch order, for the placement picker. */
export function placements(s: Series) {
  const spots: { label: string; season: number | null; number: number | null }[] = [];
  for (const season of s.seasons.filter((x) => x.number > 0 && x.episodes.length)) {
    spots.push({ label: `Before ${code(season.episodes[0])}, at the start of ${seasonName(season)}`, season: season.number, number: 0 });
    for (const e of season.episodes) spots.push({ label: `After ${code(e)}: ${e.title}`, season: season.number, number: e.number });
  }
  spots.push({ label: "After everything, with the other specials", season: null, number: null });
  return spots;
}

/** "Plays after S2 E5", or nothing for a special left at the end. */
export function placement(s: Series, e: Episode) {
  if (e.season_number !== 0 || e.after_season == null) return "";
  const season = s.seasons.find((x) => x.number === e.after_season && x.episodes.length);
  if (!season) return "";
  if (e.after_number == null) return `Plays after ${seasonName(season)}`;
  const follows = season.episodes.filter((x) => x.number <= e.after_number!).pop();
  return follows ? `Plays after ${code(follows)}` : `Plays before ${code(season.episodes[0])}`;
}

export function playLabel(s: Series) {
  const e = resumeEpisode(s);
  if (!e) return "Play";
  const all = episodesOf(s);
  if (all.every((x) => x.watched)) return `Play again ${code(e)}`;
  return `${e.position_ms > 0 ? "Resume" : "Play"} ${code(e)}`;
}

export const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export function clock(seconds: number) {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}

/** "41 min", "12 min left", or nothing until the file has been read. */
export function runtime(item: Playable) {
  if (!item.duration_ms) return "";
  const mins = (ms: number) => Math.max(1, Math.round(ms / 60000));
  if (!item.watched && item.position_ms > 0) return `${mins(item.duration_ms - item.position_ms)} min left`;
  return `${mins(item.duration_ms)} min`;
}

export const fraction = (item: Playable) => (item.duration_ms ? Math.min(1, item.position_ms / item.duration_ms) : 0);
