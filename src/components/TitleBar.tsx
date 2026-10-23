import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  createContext,
  type MouseEvent,
  type ReactNode,
  type Ref,
  useContext,
  useEffect,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { Close, Maximize, Minus, Restore } from "./Icons";
import { Logo } from "./Logo";

// The window has no native frame. Any press on the bar that isn't on a control drags the window,
// and a double press maximizes or restores it, like a native caption (Tauri's drag region does the
// same on Windows).
function dragWindow(e: MouseEvent) {
  if (e.button !== 0 || !(e.target instanceof Element) || e.target.closest("button, input, a"))
    return;
  if (e.detail === 2) void getCurrentWindow().toggleMaximize();
  else if (e.detail === 1) void getCurrentWindow().startDragging();
}

/**
 * Whether the window is maximized, re-read on every resize: maximizing also happens outside the
 * button (a double press on the bar, Win+Up, Aero Snap), and each of those resizes the window.
 */
function useMaximized() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const win = getCurrentWindow();
    let latest = 0;
    const sync = () => {
      const call = ++latest;
      void win.isMaximized().then((m) => call === latest && setMaximized(m));
    };
    sync();
    const unlisten = win.onResized(sync);
    return () => {
      latest += 1;
      void unlisten.then((stop) => stop());
    };
  }, []);
  return maximized;
}

function CaptionButton({
  label,
  onClick,
  hover,
  children,
}: {
  label: string;
  onClick: () => void;
  hover: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={`grid h-full w-11.5 place-items-center text-fg-2 transition-colors duration-(--dur-fast) ease-standard ${hover}`}
    >
      {children}
    </button>
  );
}

/** The titlebar's left slot, filled by the current view through <WindowTitle>. */
export const TitleSlotContext = createContext<HTMLElement | null>(null);

/**
 * A view's title, rendered into the titlebar (as Windows 11's own apps do), which saves a row in
 * the 680px window. `children` are controls beside it, like the Day pager; `aside` sits at the
 * column's right edge, like the Day view's update prompt.
 */
export function WindowTitle({
  title,
  meta,
  children,
  aside,
}: {
  title: string;
  meta?: string;
  children?: ReactNode;
  aside?: ReactNode;
}) {
  const slot = useContext(TitleSlotContext);
  if (!slot) return null;
  return createPortal(
    <>
      <h1 className="text-title whitespace-nowrap">{title}</h1>
      {meta && <span className="truncate text-meta text-fg-3">{meta}</span>}
      {children && <div className="ml-1 flex shrink-0 items-center gap-0.5">{children}</div>}
      {aside && <div className="ml-auto flex shrink-0 items-center gap-1">{aside}</div>}
    </>,
    slot,
  );
}

/**
 * 40px custom titlebar: the view's title slot (or the app name before onboarding), a drag region,
 * and 46×40 minimize, maximize/restore and close buttons (close destroys the window). The title
 * lines up with the current view's column (`--view-max`, centered in <main>), so the caption
 * buttons sit out of the flow at the window's corner.
 */
export function TitleBar({ slotRef, brand }: { slotRef: Ref<HTMLDivElement>; brand: boolean }) {
  const maximized = useMaximized();
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the drag region is pointer-only by nature; every control inside is a real button.
    <header
      onMouseDown={dragWindow}
      className="relative h-(--titlebar-height) shrink-0 select-none"
    >
      <div className="mx-auto flex h-full max-w-[calc(var(--view-max)+48px)] items-center gap-3 px-6">
        {brand && (
          <span className="flex items-center gap-2 text-meta text-fg-3">
            <Logo />
            Productivity Tracker
          </span>
        )}
        {/* Stops short of the 138px of caption buttons even when the column reaches them. */}
        <div ref={slotRef} className="flex min-w-0 flex-1 items-center gap-3 pr-36" />
      </div>
      <div className="absolute inset-y-0 right-0 flex">
        <CaptionButton
          label="Minimize"
          hover="hover:bg-ghost-hover hover:text-fg-1"
          onClick={() => void getCurrentWindow().minimize()}
        >
          <Minus />
        </CaptionButton>
        <CaptionButton
          label={maximized ? "Restore down" : "Maximize"}
          hover="hover:bg-ghost-hover hover:text-fg-1"
          onClick={() => void getCurrentWindow().toggleMaximize()}
        >
          {maximized ? <Restore /> : <Maximize />}
        </CaptionButton>
        <CaptionButton
          label="Close"
          hover="hover:bg-close-hover hover:text-fg-1"
          onClick={() => void getCurrentWindow().destroy()}
        >
          <Close />
        </CaptionButton>
      </div>
    </header>
  );
}
