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
export type Episode = Playable & { season_number: number; number: number; number_end: number | null };
export type Movie = Playable & { year: number | null };
export type Season = { id: number; number: number; title: string | null; episodes: Episode[] };
export type Series = { id: number; title: string; seasons: Season[]; resume_episode_id: number | null };
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
  moveEpisode: (episodeId: number, seasonNumber: number) => invoke<void>("move_episode", { episodeId, seasonNumber, number: null }),
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
export const code = (e: Episode) => `S${e.season_number} E${e.number}${e.number_end ? `-${e.number_end}` : ""}`;

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
