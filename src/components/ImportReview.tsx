import { memo, useMemo, useRef, useState } from "react";
import { type Proposed, plural } from "../api";

type Row = Proposed & { group: string };
type Patch = Partial<Proposed>;

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
const groupOf = (p: Proposed) => (p.kind === "episode" ? `series:${p.series.toLowerCase()}` : p.kind);

const Line = memo(function Line({ row, index, onPatch }: { row: Row; index: number; onPatch: (index: number, patch: Patch) => void }) {
  const number = (key: "season" | "episode", label: string) => (
    <input
      className="field num"
      type="number"
      min={0}
      step={1}
      aria-label={label}
      value={row[key]}
      onChange={(e) => onPatch(index, { [key]: Math.max(0, Math.floor(Number(e.target.value)) || 0) })}
    />
  );
  return (
    <div className={`imp-row ${row.kind === "episode" ? "" : "flat"}`}>
      <label className="tick">
        <input type="checkbox" checked={row.include} aria-label={`Add ${row.title}`} onChange={(e) => onPatch(index, { include: e.target.checked })} />
      </label>
      {row.kind === "episode" && number("season", "Season")}
      {row.kind === "episode" && number("episode", "Episode")}
      <input className="field" aria-label="Title" value={row.title} onChange={(e) => onPatch(index, { title: e.target.value })} />
      <span className="meta file" title={row.path}>
        {fileName(row.path)}
      </span>
    </div>
  );
});

/** The organiser's result, editable before anything is saved. */
export function ImportReview({ proposals, onCommit, onCancel }: { proposals: Proposed[]; onCommit: (items: Proposed[]) => void; onCancel: () => void }) {
  // Groups are fixed when the page opens, so typing a new series name doesn't reshuffle the table.
  const [rows, setRows] = useState<Row[]>(() => proposals.map((p) => ({ ...p, group: groupOf(p) })));
  const [missing, setMissing] = useState<string | null>(null);
  const [nothing, setNothing] = useState(false);
  const page = useRef<HTMLDivElement>(null);
  const patch = useRef((index: number, change: Patch) => setRows((all) => all.map((r, i) => (i === index ? { ...r, ...change } : r)))).current;
  const patchGroup = (group: string, change: Patch) => setRows((all) => all.map((r) => (r.group === group ? { ...r, ...change } : r)));

  const groups = useMemo(() => {
    const map = new Map<string, number[]>();
    rows.forEach((r, i) => map.set(r.group, [...(map.get(r.group) ?? []), i]));
    const order = (key: string) => (key.startsWith("series:") ? 0 : key === "movie" ? 1 : 2);
    return [...map].sort(([a], [b]) => order(a) - order(b));
  }, [rows]);
  const chosen = rows.filter((r) => r.include).length;

  const commit = () => {
    if (!chosen) return setNothing(true);
    const unnamed = rows.find((r) => r.include && r.kind === "episode" && !r.series.trim());
    if (unnamed) {
      setMissing(unnamed.group);
      page.current?.querySelector<HTMLInputElement>(`[data-group="${CSS.escape(unnamed.group)}"]`)?.focus();
      return;
    }
    onCommit(rows.map(({ group: _group, ...item }) => item));
  };

  return (
    <div className="import" ref={page}>
      <div className="page">
        <div className="head">
          <h2>Check what was found</h2>
          <span className="small">Sorted from the file names. Change anything that looks wrong. Your files are not moved or renamed.</span>
        </div>
        {groups.map(([group, indexes]) => {
          const first = rows[indexes[0]];
          const included = indexes.filter((i) => rows[i].include).length;
          const isSeries = first.kind === "episode";
          return (
            <section className="imp-group" key={group}>
              <div className="imp-head">
                <label className="tick">
                  <input
                    type="checkbox"
                    checked={included === indexes.length}
                    aria-label={`Add everything in ${isSeries ? first.series : group}`}
                    onChange={(e) => patchGroup(group, { include: e.target.checked })}
                  />
                </label>
                {isSeries ? (
                  <label className="name">
                    <span className="meta">Series name</span>
                    <input
                      className="field"
                      data-group={group}
                      value={first.series}
                      aria-invalid={missing === group}
                      aria-describedby={missing === group ? "series-name-error" : undefined}
                      onChange={(e) => (setMissing(null), patchGroup(group, { series: e.target.value }))}
                      onBlur={(e) => !e.target.value.trim() && included > 0 && setMissing(group)}
                    />
                    {missing === group && (
                      <span className="error" id="series-name-error">
                        This series needs a name before it can be added. Type one here.
                      </span>
                    )}
                  </label>
                ) : (
                  <div className="name">
                    <h3>{group === "movie" ? "Movies" : "Not sure about these"}</h3>
                    {group === "unsorted" && <span className="small">No episode number or year in the name. Tick any you want added as movies.</span>}
                  </div>
                )}
                <span className="small num">
                  {included} of {plural(indexes.length, "file")}
                </span>
              </div>
              <div className={`imp-row cols ${isSeries ? "" : "flat"}`} aria-hidden="true">
                <span>Add</span>
                {isSeries && <span>Season</span>}
                {isSeries && <span>Episode</span>}
                <span>Title</span>
                <span>File</span>
              </div>
              {indexes.map((i) => (
                <Line key={rows[i].path} row={rows[i]} index={i} onPatch={patch} />
              ))}
            </section>
          );
        })}
      </div>
      <div className="imp-bar">
        {nothing && !chosen ? (
          <span className="error" role="alert">
            Nothing is ticked. Tick the files you want to add, or cancel.
          </span>
        ) : (
          <span className="small num">{plural(chosen, "file")} selected</span>
        )}
        <div className="actions">
          <button type="button" className="btn quiet" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="btn primary" onClick={commit}>
            {chosen ? `Add ${plural(chosen, "file")}` : "Add files"}
          </button>
        </div>
      </div>
    </div>
  );
}
