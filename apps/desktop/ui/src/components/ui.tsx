import { useId, useState, type ReactNode } from "react";

/**
 * Accessible inline help tooltip (Phase 13): shows on hover, keyboard focus,
 * or click/tap (for touch). docs/08 rule: never put secrets in tooltips.
 * Renders a small "?" trigger inline after a label.
 */
export function Tooltip({ label }: { label: string }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <span className="relative ml-1 inline-flex">
      <button
        type="button"
        aria-describedby={open ? id : undefined}
        aria-label={`Help: ${label}`}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={(e) => {
          e.preventDefault();
          setOpen((o) => !o);
        }}
        className="inline-flex h-4 w-4 items-center justify-center rounded-full border border-zinc-400 text-[10px] font-semibold leading-none text-zinc-500 hover:border-zinc-600 hover:text-zinc-700 dark:border-zinc-600 dark:text-zinc-400 dark:hover:text-zinc-200"
      >
        ?
      </button>
      {open && (
        <span
          role="tooltip"
          id={id}
          className="absolute left-1/2 top-6 z-50 w-64 -translate-x-1/2 rounded-md border border-zinc-200 bg-white px-2.5 py-2 text-xs font-normal normal-case text-zinc-700 shadow-lg dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200"
        >
          {label}
        </span>
      )}
    </span>
  );
}

/** Wraps a form label's text with an inline help tooltip. */
export function FieldLabel({ text, help }: { text: string; help: string }) {
  return (
    <span className="inline-flex items-center">
      {text}
      <Tooltip label={help} />
    </span>
  );
}

/** Inline helper paragraph under a field or section header. */
export function Hint({ children }: { children: ReactNode }) {
  return (
    <p className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">{children}</p>
  );
}

/** Small loading spinner for buttons and empty/loading panels. */
export function Spinner({ className = "" }: { className?: string }) {
  return (
    <span
      role="status"
      aria-label="Loading"
      className={
        "inline-block h-4 w-4 animate-spin rounded-full border-2 border-zinc-300 border-t-[#006eb9] dark:border-zinc-700 dark:border-t-sky-400 " +
        className
      }
    />
  );
}

/** Empty-state panel: explains why it's blank + one CTA (research: teach & convert). */
export function EmptyState({
  title,
  body,
  action,
}: {
  title: string;
  body: string;
  action?: ReactNode;
}) {
  return (
    <div className="mt-3 rounded-lg border border-dashed border-zinc-300 p-4 text-sm dark:border-zinc-700">
      <p className="font-medium">{title}</p>
      <p className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">{body}</p>
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
