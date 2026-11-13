import { useCallback, useEffect, useState } from "react";
import { type WatchFolder, api, pickFolder, plural } from "../api";
import { Icon } from "./ui";

type Props = {
  onFindMoved: () => void;
  onChanged: () => void;
  onNotice: (text: string) => void;
  onError: (message: string) => void;
};

const SHORTCUTS: [string, string][] = [
  ["Play or pause", "Space"],
  ["Back 10 seconds", "Left"],
  ["Forward 10 seconds", "Right"],
  ["Volume up or down", "Up / Down, or the mouse wheel"],
  ["Mute", "M"],
  ["Fullscreen", "F"],
  ["Next episode", "N"],
  ["Episodes", "E"],
  ["Audio and subtitles", "S"],
  ["Next audio track", "A"],
  ["Slower or faster", "[ / ]"],
  ["Playback details", "I"],
  ["Leave the player", "Esc"],
  ["Search the library", "Ctrl K"],
];

export function Settings({ onFindMoved, onChanged, onNotice, onError }: Props) {
  const [folders, setFolders] = useState<WatchFolder[]>([]);
  const [adding, setAdding] = useState(false);
  const load = useCallback(() => api.watchFolders().then(setFolders, (e) => onError(String(e))), [onError]);
  useEffect(() => void load(), [load]);

  const add = async () => {
    try {
      const path = await pickFolder("Watch a folder");
      if (!path) return;
      setAdding(true);
      const added = await api.addWatchFolder(path);
      await load();
      onChanged();
      onNotice(added ? `Watching ${path}. Added ${plural(added, "file")} from it.` : `Watching ${path}. Nothing new in it yet.`);
    } catch (e) {
      onError(String(e));
    } finally {
      setAdding(false);
    }
  };
  const stop = (folder: WatchFolder) => api.removeWatchFolder(folder.id).then(load, (e) => onError(String(e)));

  return (
    <div className="page settings">
      <h1>Settings</h1>

      <section aria-labelledby="set-watch">
        <h2 id="set-watch">Watch folders</h2>
        <p>New video files that appear in these folders are sorted into your library by themselves. Anything you removed from the library stays removed.</p>
        <div className="frame">
          {folders.map((f) => (
            <div className="line" key={f.id}>
              <span className="path" title={f.path}>
                {f.path}
              </span>
              <button type="button" className="link" aria-label={`Stop watching ${f.path}`} onClick={() => stop(f)}>
                Stop watching
              </button>
            </div>
          ))}
          {!folders.length && <div className="none">No folders are watched. Add the folder your downloads land in.</div>}
        </div>
        <div className="actions">
          <button type="button" className="btn quiet" disabled={adding} onClick={add}>
            <Icon name="plus" />
            {adding ? "Adding the folder" : "Watch a folder"}
          </button>
        </div>
      </section>

      <section aria-labelledby="set-moved">
        <h2 id="set-moved">Moved files</h2>
        <p>If you moved or renamed video files, pick the folder they are in now. Marquee matches them to your library and keeps their names and watch progress.</p>
        <div className="actions">
          <button type="button" className="btn quiet" onClick={onFindMoved}>
            Find moved files
          </button>
        </div>
      </section>

      <section aria-labelledby="set-keys">
        <h2 id="set-keys">Keyboard shortcuts</h2>
        <dl className="keys">
          {SHORTCUTS.map(([what, key]) => (
            <div key={what}>
              <dt>{what}</dt>
              <dd>
                <kbd>{key}</kbd>
              </dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  );
}
