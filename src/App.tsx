import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { type Library, type Proposed, type Series, SUBS, api, itemOf, pickFile, pickFiles, pickFolders, plural } from "./api";
import { CommandBar } from "./components/CommandBar";
import { EmptyLibrary, Home } from "./components/Home";
import { ImportReview } from "./components/ImportReview";
import { Player } from "./components/Player";
import { SeriesPage } from "./components/SeriesPage";
import { Settings } from "./components/Settings";
import { Icon, MissingFile } from "./components/ui";

/** `scan` numbers each import so a second drop replaces the table instead of being ignored. */
type View = { name: "home" } | { name: "settings" } | { name: "series"; id: number } | { name: "import"; proposals: Proposed[]; scan: number; back: View };
type Notice = { text: string; error: boolean };

const SUBTITLE = new RegExp(`\\.(${SUBS.join("|")})$`, "i");

export default function App() {
  const [lib, setLib] = useState<Library | null>(null);
  const [view, setView] = useState<View>({ name: "home" });
  const [playing, setPlaying] = useState<number | null>(null);
  const [searching, setSearching] = useState(false);
  const [dropping, setDropping] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  // Kept apart from the notice, which clears itself after a few seconds.
  const [loadError, setLoadError] = useState<string | null>(null);
  const main = useRef<HTMLElement>(null);
  const scans = useRef(0);
  // What had focus when playback started, so leaving the player puts it back.
  const opener = useRef<HTMLElement | null>(null);
  // A file that could not be played because it is not at its path any more.
  const [missing, setMissing] = useState<number | null>(null);
  const live = useRef({ playing, view, lib });
  live.current = { playing, view, lib };

  const fail = useCallback((text: string) => setNotice({ text, error: true }), []);
  const reload = useCallback(
    () =>
      api.library().then(
        (l) => (setLib(l), setLoadError(null)),
        // With a library already on screen this shows as a notice; with none, as the full screen below.
        (e) => (setLoadError(String(e)), fail(String(e))),
      ),
    [fail],
  );

  useEffect(() => {
    reload();
    // Thumbnails arrive one file at a time; refresh at most a few times a second.
    let timer: number | undefined;
    const unlisten = listen("library-changed", () => {
      clearTimeout(timer);
      timer = window.setTimeout(reload, 400);
    });
    const unwatch = listen<number>("watch-added", (e) => setNotice({ text: `Added ${plural(e.payload, "file")} from your watch folders.`, error: false }));
    return () => {
      clearTimeout(timer);
      unlisten.then((off) => off());
      unwatch.then((off) => off());
    };
  }, [reload]);

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), 8000);
    return () => clearTimeout(timer);
  }, [notice]);

  // While something plays the page turns see-through, so the video behind the webview shows.
  useEffect(() => {
    document.documentElement.classList.toggle("playing", playing != null);
    // Back in the library (no longer inert): hand focus to what started playback.
    if (playing == null && opener.current?.isConnected) opener.current.focus();
    if (playing == null) opener.current = null;
  }, [playing]);

  useEffect(() => {
    main.current?.scrollTo(0, 0);
  }, [view.name]);

  const review = useCallback(
    async (paths: string[], adjust?: (found: Proposed[]) => Proposed[]) => {
      if (!paths.length) return;
      try {
        const found = await api.importScan(paths);
        if (!found.length) return setNotice({ text: "No video files were found in what you picked. Marquee adds MKV, MP4, AVI, MOV, WebM and similar files.", error: true });
        const current = live.current.view;
        setView({ name: "import", proposals: adjust ? adjust(found) : found, scan: ++scans.current, back: current.name === "import" ? current.back : current });
      } catch (e) {
        fail(String(e));
      }
    },
    [fail],
  );
  const add = useCallback((what: "files" | "folder") => void (what === "folder" ? pickFolders() : pickFiles()).then((paths) => review(paths), (e) => fail(String(e))), [review, fail]);

  const addToSeries = (series: Series, seasonNumber: number) =>
    void pickFiles().then((paths) =>
      review(paths, (found) =>
        found.map((p, i) => ({
          ...p,
          kind: "episode",
          series: series.title,
          season: p.kind === "episode" ? p.season : seasonNumber,
          episode: p.kind === "episode" ? p.episode : i + 1,
          include: true,
        })),
      ),
    );

  const commit = async (items: Proposed[], back: View) => {
    try {
      const added = await api.importCommit(items);
      await reload();
      setView(back);
      setNotice(added ? { text: `Added ${plural(added, "file")} to your library.`, error: false } : { text: "Nothing new was added. Those files are already in your library.", error: false });
    } catch (e) {
      fail(String(e));
    }
  };

  const play = useCallback((mediaFileId: number) => {
    if (live.current.playing == null) opener.current = document.activeElement as HTMLElement | null;
    void api.play(mediaFileId).then((started) => void (started && (setPlaying(mediaFileId), reload())), (e) =>
      // A file that has gone gets the dialog that can find it again; anything else is a plain notice.
      api.fileMissing(mediaFileId).then((gone) => (gone ? setMissing(mediaFileId) : fail(String(e))), () => fail(String(e))),
    );
  }, [reload, fail]);

  /** Point one library item at a file picked by hand, then play it if that was the aim. */
  const locate = useCallback(
    async (mediaFileId: number, thenPlay: boolean) => {
      try {
        const path = await pickFile("Locate file");
        if (!path) return;
        await api.relinkFile(mediaFileId, path);
        await reload();
        setMissing(null);
        if (thenPlay) play(mediaFileId);
        else setNotice({ text: "The library now points at that file. Its name and watch progress are unchanged.", error: false });
      } catch (e) {
        fail(String(e));
      }
    },
    [reload, fail, play],
  );
  const findMoved = useCallback(async () => {
    try {
      const paths = await pickFolders("Search a folder for moved files");
      if (!paths.length) return;
      const [found, still] = await api.relinkFolder(paths);
      await reload();
      setMissing(null);
      const left = still ? ` ${plural(still, "file")} still missing.` : "";
      setNotice({
        text: found ? `Found ${plural(found, "moved file")}.${left}` : still ? `None of the ${plural(still, "missing file")} turned up in that folder.` : "Nothing is missing. Every file in your library is where Marquee expects it.",
        error: false,
      });
    } catch (e) {
      fail(String(e));
    }
  }, [reload, fail]);
  const stop = useCallback(async () => {
    try {
      await api.stop();
      if (await getCurrentWindow().isFullscreen()) await api.toggleFullscreen();
    } catch (e) {
      fail(String(e));
    }
    setPlaying(null);
    reload();
  }, [reload, fail]);

  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") setDropping(true);
      else setDropping(false);
      if (payload.type !== "drop") return;
      // A subtitle file dropped on the player is loaded into what is playing.
      if (live.current.playing != null) {
        const subtitle = payload.paths.find((p) => SUBTITLE.test(p));
        if (subtitle) api.addSubtitle(subtitle).catch((e) => fail(String(e)));
        else setNotice({ text: "Only subtitle files can be dropped while something is playing. Go back to the library to add videos.", error: false });
        return;
      }
      review(payload.paths);
    });
    return () => void unlisten.then((off) => off());
  }, [review, fail]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "k" && live.current.playing == null) {
        e.preventDefault();
        setSearching(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const toast = notice && (
    <div className="toast" role={notice.error ? "alert" : "status"}>
      <span>{notice.text}</span>
      <button type="button" className="link" onClick={() => setNotice(null)}>
        Dismiss
      </button>
    </div>
  );
  // The library could not be read: say so and offer another go, rather than a black window.
  if (!lib)
    return (
      loadError && (
        <div className="empty">
          <h1>The library did not open</h1>
          <p>{loadError}</p>
          <div className="actions">
            <button type="button" className="btn primary" onClick={() => (setNotice(null), reload())}>
              Try again
            </button>
          </div>
        </div>
      )
    );
  const series = view.name === "series" ? lib.series.find((s) => s.id === view.id) : undefined;
  const empty = !lib.series.length && !lib.movies.length;
  const home = () => setView({ name: "home" });
  const gone = missing != null ? itemOf(lib, missing) : undefined;

  return (
    <>
      <div className="app" inert={playing != null}>
        <nav className="rail" aria-label="Main">
          <div className="mark" aria-hidden="true">
            M
          </div>
          <button type="button" data-tip="Home" aria-label="Home" aria-current={view.name === "home" ? "page" : undefined} onClick={home}>
            <Icon name="home" />
          </button>
          <button type="button" data-tip="Search (Ctrl K)" aria-label="Search" onClick={() => setSearching(true)}>
            <Icon name="search" />
          </button>
          <div className="spacer" />
          <button type="button" data-tip="Add folder" aria-label="Add folder" onClick={() => add("folder")}>
            <Icon name="plus" />
          </button>
          <button type="button" data-tip="Settings" aria-label="Settings" aria-current={view.name === "settings" ? "page" : undefined} onClick={() => setView({ name: "settings" })}>
            <Icon name="settings" />
          </button>
        </nav>
        <main ref={main}>
          <div className="view" key={view.name === "series" ? `series${view.id}` : view.name}>
            {view.name === "import" ? (
              <ImportReview key={view.scan} proposals={view.proposals} onCancel={() => setView(view.back)} onCommit={(items) => commit(items, view.back)} />
            ) : view.name === "settings" ? (
              <Settings onFindMoved={findMoved} onChanged={reload} onNotice={(text) => setNotice({ text, error: false })} onError={fail} />
            ) : series ? (
              <SeriesPage lib={lib} series={series} onBack={home} onPlay={play} onAddFiles={addToSeries} onLocate={(id) => locate(id, false)} onFindMoved={findMoved} onChanged={reload} onError={fail} />
            ) : empty ? (
              <EmptyLibrary onAdd={add} />
            ) : (
              <Home lib={lib} onOpen={(s) => setView({ name: "series", id: s.id })} onPlay={play} onAdd={add} onSearch={() => setSearching(true)} onChanged={reload} onError={fail} />
            )}
          </div>
        </main>
      </div>

      {playing != null && <Player lib={lib} mediaFileId={playing} onPlay={play} onClose={stop} onError={fail} />}

      {searching && (
        <CommandBar
          lib={lib}
          onClose={() => setSearching(false)}
          onOpenSeries={(id) => setView({ name: "series", id })}
          onPlay={play}
          actions={[
            { id: "kbar-add-folder", label: "Add folder", hint: "Action", run: () => add("folder") },
            { id: "kbar-add-files", label: "Add files", hint: "Action", run: () => add("files") },
          ]}
        />
      )}

      {gone && (
        <MissingFile name={gone.title} path={gone.path} onLocate={() => locate(gone.media_file_id, true)} onSearch={findMoved} onClose={() => setMissing(null)} />
      )}

      {dropping && playing == null && (
        <div className="drop" aria-hidden="true">
          <span>Drop to add to your library</span>
        </div>
      )}

      {toast}
    </>
  );
}
