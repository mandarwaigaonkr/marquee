import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";

const PATHS: Record<string, ReactNode> = {
  home: <path d="M4 5h4v14H4zM11 5h9v14h-9z" />,
  search: (
    <>
      <circle cx="11" cy="11" r="6" />
      <path d="M20 20l-4.500-4.500" />
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" />,
  back: <path d="M15 5l-7 7 7 7" />,
  close: <path d="M6 6l12 12M18 6L6 18" />,
  check: <path d="M5 12.500l4.500 4.500L19 7.500" />,
  more: <path d="M5 12h.01M12 12h.01M19 12h.01" strokeWidth="3" />,
  pause: <path d="M8 5v14M16 5v14" />,
  rewind: <path d="M4 12a8 8 0 1 0 3-6.200M4 4v4h4" />,
  forward: <path d="M20 12a8 8 0 1 1-3-6.200M20 4v4h-4" />,
  volume: <path d="M4 10v4h4l5 4V6L8 10zM17 9v6" />,
  muted: <path d="M4 10v4h4l5 4V6L8 10zM16 10l4 4M20 10l-4 4" />,
  next: <path d="M6 5l10 7-10 7zM19 5v14" />,
  list: <path d="M4 6h16M4 12h16M4 18h16" />,
  subs: <path d="M3 5h18v14H3zM7 14h4M13 14h4M7 10h2" />,
  speed: <path d="M5 17a8 8 0 1 1 14 0M12 13l4-5" />,
  fullscreen: <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />,
  minus: <path d="M5 12h14" />,
  chevron: <path d="M9 5l7 7-7 7" />,
  settings: (
    <>
      <path d="M4 7h9M17 7h3M4 17h3M11 17h9" />
      <path d="M13 5h4v4h-4zM7 15h4v4H7z" />
    </>
  ),
};

export function Icon({ name }: { name: string }) {
  if (name === "play")
    return (
      <svg className="i solid" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M7 4.500v15l13-7.500z" />
      </svg>
    );
  return (
    <svg className="i" viewBox="0 0 24 24" aria-hidden="true">
      {PATHS[name]}
    </svg>
  );
}

/** Square icon button; the tooltip doubles as its accessible name. */
export function IconButton({ icon, tip, label, onClick, className = "" }: { icon: string; tip: string; label?: string; onClick: () => void; className?: string }) {
  return (
    <button type="button" className={`icon-btn ${className}`} data-tip={tip} aria-label={label ?? tip.replace(/ \(.*\)$/, "")} onClick={onClick}>
      <Icon name={icon} />
    </button>
  );
}

/** A frame from one of the user's files. It fades in once loaded, so rows never pop. */
export function Frame({ src, lazy }: { src: string; lazy?: boolean }) {
  return <img className="frame-img" src={src} alt="" loading={lazy ? "lazy" : undefined} onLoad={(e) => e.currentTarget.classList.add("in")} />;
}

export function Thumb({ src }: { src: string | undefined }) {
  return <span className="thumb">{src && <Frame src={src} lazy />}</span>;
}

export function Progress({ value }: { value: number }) {
  return (
    <span className="progress">
      <i style={{ transform: `scaleX(${value})` }} />
    </span>
  );
}

/** Text that turns into a field in the same box, so nothing around it moves. */
export function InlineEdit({ value, editing, label, fit, onDone }: { value: string; editing: boolean; label: string; fit?: boolean; onDone: (next: string | null) => void }) {
  const ref = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  const [draft, setDraft] = useState(value);
  useLayoutEffect(() => {
    if (editing) {
      done.current = false;
      setDraft(value);
      ref.current?.select();
    }
  }, [editing]);
  if (!editing) return <span className="ie">{value}</span>;
  const finish = (next: string | null) => {
    if (done.current) return;
    done.current = true;
    // Focus would otherwise fall to the page when the field goes away.
    const home = ref.current?.closest(".row, .series-top, .tabs");
    requestAnimationFrame(() => home?.querySelector<HTMLElement>('.menu > button, [role="tab"][aria-selected="true"]')?.focus());
    const text = next?.trim();
    onDone(text && text !== value ? text : null);
  };
  return (
    <span className={fit ? "ie fit" : "ie"}>
      {fit && (
        <span className="sizer" aria-hidden="true">
          {draft || "\u00a0"}
        </span>
      )}
      <input
        ref={ref}
        defaultValue={value}
        onChange={(e) => setDraft(e.currentTarget.value)}
        aria-label={label}
        onClick={(e) => e.stopPropagation()}
        onBlur={(e) => finish(e.currentTarget.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") finish(e.currentTarget.value);
          if (e.key === "Escape") finish(null);
        }}
      />
    </span>
  );
}

export type MenuItem = { label: string; danger?: boolean; onSelect?: () => void; items?: MenuItem[] };

/** Small anchored menu with one optional nested level. */
export function Menu({ trigger, label, items, className = "" }: { trigger: ReactNode; label: string; items: MenuItem[]; className?: string }) {
  const [open, setOpen] = useState(false);
  const [sub, setSub] = useState<MenuItem | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const close = (refocus: boolean) => {
    setOpen(false);
    setSub(null);
    if (refocus) button.current?.focus();
  };
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => !root.current?.contains(e.target as Node) && close(false);
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  useEffect(() => {
    if (open) root.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
  }, [open, sub]);
  const onKey = (e: React.KeyboardEvent) => {
    if (!open) return;
    const all = [...(root.current?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
    const at = all.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Escape") {
      e.stopPropagation();
      close(true);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      all[(at + (e.key === "ArrowDown" ? 1 : all.length - 1)) % all.length]?.focus();
    } else if (e.key === "Tab") close(false);
  };
  return (
    <div className={`menu ${className}`} ref={root} onKeyDown={onKey}>
      <button
        type="button"
        ref={button}
        className={typeof trigger === "string" ? "btn quiet" : "icon-btn"}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        onClick={(e) => {
          e.stopPropagation();
          open ? close(false) : setOpen(true);
        }}
      >
        {trigger}
      </button>
      {open && (
        <div className="menu-list" role="menu" aria-label={label}>
          {sub && <div className="menu-title">{sub.label}</div>}
          {(sub?.items ?? items).map((item) => (
            <button
              type="button"
              role="menuitem"
              key={item.label}
              className={item.danger ? "danger" : ""}
              onClick={(e) => {
                e.stopPropagation();
                if (item.items) return setSub(item);
                close(true);
                item.onSelect?.();
              }}
            >
              {item.label}
              {item.items && <Icon name="chevron" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** Confirmation that names the thing and the action. Native dialog traps and restores focus. */
export function Confirm({ title, body, action, onConfirm, onCancel }: { title: string; body: string; action: string; onConfirm: () => void; onCancel: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  // Layout effect: close() must run while the dialog is still in the page, or focus is not handed back.
  useLayoutEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return (
    <dialog ref={ref} className="confirm" onCancel={onCancel} onClick={(e) => e.target === ref.current && onCancel()}>
      <div className="confirm-body">
        <h2>{title}</h2>
        <p>{body}</p>
        <div className="actions">
          <button type="button" className="btn quiet" autoFocus onClick={onCancel}>
            Keep it
          </button>
          <button type="button" className="btn danger" onClick={onConfirm}>
            {action}
          </button>
        </div>
      </div>
    </dialog>
  );
}

/** Shown when a file is not where the library says it is. */
export function MissingFile({ name, path, onLocate, onSearch, onClose }: { name: string; path: string; onLocate: () => void; onSearch: () => void; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    // showModal focuses the first button; the useful one is the last.
    dialog?.querySelector<HTMLElement>("[data-first]")?.focus();
    return () => dialog?.close();
  }, []);
  return (
    <dialog ref={ref} className="confirm" onCancel={onClose} onClick={(e) => e.target === ref.current && onClose()}>
      <div className="confirm-body">
        <h2>{name} is not where it was</h2>
        <p>
          Marquee looked for it at <span className="where">{path}</span>. It may have been moved or renamed, or its drive is unplugged. Point to the file, or pick a folder to search for everything that moved.
        </p>
        <div className="actions">
          <button type="button" className="btn quiet" onClick={onClose}>
            Close
          </button>
          <button type="button" className="btn quiet" onClick={onSearch}>
            Search a folder
          </button>
          <button type="button" className="btn primary" data-first onClick={onLocate}>
            Locate file
          </button>
        </div>
      </div>
    </dialog>
  );
}

/** The clickable body of a list row. While its title is being renamed it is a plain box, since a field can't live inside a button. */
export function RowMain({ editing, label, onClick, children }: { editing: boolean; label: string; onClick: () => void; children: ReactNode }) {
  if (editing) return <div className="row-main">{children}</div>;
  return (
    <button type="button" className="row-main" aria-label={label} onClick={onClick}>
      {children}
    </button>
  );
}
