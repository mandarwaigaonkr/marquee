import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { type Library, playLabel } from "../api";
import { Icon } from "./ui";

export type Command = { id: string; label: string; hint: string; run: () => void };

/** Ctrl+K: type a few letters to jump to a series, a movie or an action. */
export function CommandBar({ lib, actions, onClose, onOpenSeries, onPlay }: { lib: Library; actions: Command[]; onClose: () => void; onOpenSeries: (id: number) => void; onPlay: (mediaFileId: number) => void }) {
  const [query, setQuery] = useState("");
  const [at, setAt] = useState(0);
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  // Layout effect: close() must run while the dialog is still in the page, or focus is not handed back.
  useLayoutEffect(() => {
    const el = dialog.current;
    el?.showModal();
    return () => el?.close();
  }, []);

  const q = query.trim().toLowerCase();
  const results: Command[] = [
    ...lib.series.map((s) => ({ id: `kbar-s${s.id}`, label: s.title, hint: playLabel(s), run: () => onOpenSeries(s.id) })),
    ...lib.movies.map((m) => ({ id: `kbar-m${m.id}`, label: m.title, hint: "Movie", run: () => onPlay(m.media_file_id) })),
    ...actions,
  ].filter((c) => c.label.toLowerCase().includes(q));
  const selected = Math.min(at, results.length - 1);
  const active = results[selected]?.id;
  // Arrowing past the fold keeps the highlighted row in sight.
  useEffect(() => {
    if (active) document.getElementById(active)?.scrollIntoView({ block: "nearest" });
  }, [active]);
  const choose = (c: Command) => {
    onClose();
    c.run();
  };

  return (
    <dialog
      ref={dialog}
      className="kbar"
      aria-label="Jump to"
      onCancel={onClose}
      onClick={(e) => e.target === dialog.current && onClose()}
      onKeyDown={(e) => {
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          setAt((selected + (e.key === "ArrowDown" ? 1 : results.length - 1)) % Math.max(1, results.length));
        } else if (e.key === "Enter" && results[selected] && e.target === input.current) choose(results[selected]);
      }}
    >
      <label className="kq">
        <Icon name="search" />
        <span>Jump to</span>
        <input ref={input} autoFocus autoComplete="off" role="combobox" aria-expanded="true" aria-controls="kbar-results" aria-activedescendant={active} value={query} onChange={(e) => (setQuery(e.target.value), setAt(0))} />
      </label>
      <ul role="listbox" id="kbar-results" aria-label="Results">
        {results.map((c, i) => (
          <li key={c.id} id={c.id} role="option" aria-selected={i === selected} className={i === selected ? "sel" : ""} onPointerMove={() => setAt(i)} onClick={() => choose(c)}>
            <span>{c.label}</span>
            <span>{c.hint}</span>
          </li>
        ))}
        {!results.length && (
          <li className="none">
            <span>Nothing in your library matches "{query}".</span>
            <button type="button" className="clear" onClick={() => (setQuery(""), input.current?.focus())}>
              Clear search
            </button>
          </li>
        )}
      </ul>
    </dialog>
  );
}
