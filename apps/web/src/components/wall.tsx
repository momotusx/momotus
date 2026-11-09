/** First run's poster wall and its sheets (plans/first-screens.md 3.1, round2.html combo A "Poster Wall"): columns of
 * the default catalog's posters, tilted, drifting on Welcome only and held still under every sheet. The drift also stops
 * for reduced motion, a hidden page and the Pause motion toggle; its phase is kept for the app run, so Welcome resumes
 * where it stopped. A tile whose poster has not come is the empty rounded tile, never a broken image. Decorative:
 * hidden from assistive tech and never focusable. */

import Pause from "lucide-solid/icons/pause";
import Play from "lucide-solid/icons/play";
import { createEffect, createSignal, For, type JSX, onCleanup, Show } from "solid-js";
import { op } from "../core/client";
import { form } from "../form";
import { resetHistoryBase } from "../history";
import { firstRunWords as w } from "../lib/first-run";
import "./wall.css";

// The posters, kept for the app run once a non-empty answer came; an empty or failed one is asked again at the next step.
const [posters, setPosters] = createSignal<string[]>([]);
let asking: Promise<void> | null = null;
/** Asks `firstRunWall` unless the posters are in or a question is out. Only first run and the sign-in sheets ask. */
export function askWall() {
  if (posters().length || asking) return;
  asking = op<string[]>("firstRunWall")
    .then((list) => {
      if (Array.isArray(list) && list.length) setPosters(list);
    })
    .catch(() => undefined)
    .finally(() => {
      asking = null;
    });
}

// The person's Pause motion, kept on this device (a local setting, never synced).
const PAUSED = "mq.wallPaused";
const stored = () => {
  try {
    return localStorage.getItem(PAUSED) === "1";
  } catch {
    return false;
  }
};
const [paused, setPausedSignal] = createSignal(stored());
function setPaused(on: boolean) {
  setPausedSignal(on);
  try {
    if (on) localStorage.setItem(PAUSED, "1");
    else localStorage.removeItem(PAUSED);
  } catch {
    // Kept for this run only.
  }
}

// The drift's phase: seconds it has run, kept across steps and pages for the app run.
let ran = 0;
let since: number | null = null;
const phase = () => ran + (since === null ? 0 : (performance.now() - since) / 1000);

const [hidden, setHidden] = createSignal(typeof document !== "undefined" && document.hidden);
if (typeof document !== "undefined") document.addEventListener("visibilitychange", () => setHidden(document.hidden));

// The walk builds' line (plans/first-screens.md 3.1, 9), once per app run: ms from the wall's first frame to the first
// poster drawn, and the tiles filled at 6 s over the tiles in view. The bench reads it from the console.
let settleWatched = false;
let firstPoster: number | null = null;
function watchSettle() {
  // Walk builds only (development and `VITE_WALK`): nobody else's console gets the line.
  if (settleWatched || !(import.meta.env.DEV || import.meta.env.VITE_WALK)) return;
  settleWatched = true;
  const t0 = performance.now();
  const seen = (e: Event) => {
    if (firstPoster === null && e.target instanceof HTMLImageElement && e.target.closest(".wall-tile")) firstPoster = Math.round(performance.now() - t0);
  };
  document.addEventListener("load", seen, true);
  setTimeout(() => {
    document.removeEventListener("load", seen, true);
    const inView = [...document.querySelectorAll(".wall-tile")].filter((t) => {
      const r = t.getBoundingClientRect();
      return r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth;
    });
    const filled = inView.filter((t) => t.querySelector("img.in")).length;
    console.info(`first-wall poster ${firstPoster ?? -1} tiles6s ${filled}/${inView.length}`);
  }, 6000);
}

/** Leaving first run for Home (3.6): the wall and its sheet fade out in 260 ms (none for reduced motion), then Home
 * opens as the root, so Back never walks into first run. */
export function intoHome(navigate: (to: string, o?: { replace?: boolean }) => void) {
  const go = () => {
    resetHistoryBase();
    navigate("/", { replace: true });
  };
  const page = document.querySelector(".first.page");
  if (!page || matchMedia("(prefers-reduced-motion: reduce)").matches) return go();
  page.classList.add("leaving");
  setTimeout(go, 260);
}

/** Posters per column, each drawn twice so the loop is seamless. */
const PER_COLUMN = 6;

/** The wall itself. `moving`: this step drifts (Welcome); every sheet holds it still. */
export function Wall(props: { moving: boolean }) {
  askWall();
  watchSettle();
  const columns = () => (form() === "phone" ? 4 : 9);
  const runs = () => props.moving && !paused() && !hidden();
  // The phase at mount: each column starts where the drift stopped, whatever page drew it before.
  const start = phase();
  createEffect(() => {
    if (runs()) since ??= performance.now();
    else if (since !== null) {
      ran = phase();
      since = null;
    }
  });
  onCleanup(() => {
    if (since !== null) {
      ran = phase();
      since = null;
    }
  });
  const listFor = (c: number) => {
    const all = posters();
    if (!all.length) return Array.from({ length: PER_COLUMN }, () => "");
    const at = (c * 4) % all.length;
    const turned = [...all.slice(at), ...all.slice(0, at)];
    return Array.from({ length: PER_COLUMN }, (_, i) => turned[i % turned.length] as string);
  };
  return (
    <div class="wall-frame" aria-hidden="true" classList={{ still: !runs() }}>
      <div class="wall">
        <For each={Array.from({ length: columns() }, (_, c) => c)}>
          {(c) => (
            <div class="wall-col" style={{ "animation-duration": `${70 + (c % 3) * 14}s`, "animation-delay": `-${start}s` }}>
              <For each={[...listFor(c), ...listFor(c)]}>{(src) => <Tile src={src} />}</For>
            </div>
          )}
        </For>
      </div>
    </div>
  );
}

/** One poster: the empty tile at once, the poster faded in once loaded, nothing on a failure. */
function Tile(props: { src: string }) {
  const [broken, setBroken] = createSignal(false);
  return (
    <div class="wall-tile">
      <Show when={props.src && !broken()}>
        <img src={props.src} alt="" decoding="async" referrerpolicy="no-referrer" onLoad={(e) => e.currentTarget.classList.add("in")} onError={() => setBroken(true)} />
      </Show>
    </div>
  );
}

/** Welcome's Pause motion toggle: a 48 px icon button at the top right (WCAG 2.2.2). */
export function MotionToggle() {
  return (
    <button
      class="wall-pause"
      data-f="welcome:motion"
      aria-pressed={paused()}
      aria-label={w.pauseMotion}
      title={paused() ? w.playMotion : w.pauseMotion}
      onClick={() => setPaused(!paused())}
    >
      <Show when={paused()} fallback={<Pause aria-hidden />}>
        <Play aria-hidden />
      </Show>
    </button>
  );
}

// How many wall pages are drawn: the shell shows no bar or tabs over them.
const [walls, setWalls] = createSignal(0);
export const wallShown = () => walls() > 0;

/** A page of first run: the wall behind, the shade (Welcome) or the dim (a sheet), then the step. */
export function WallPage(props: { moving?: boolean; label?: string; children: JSX.Element }) {
  setWalls((n) => n + 1);
  onCleanup(() => setWalls((n) => n - 1));
  return (
    <main class="first page" classList={{ sheeted: !props.moving }} aria-label={props.label}>
      <Wall moving={!!props.moving} />
      <div class="wall-shade" aria-hidden="true" />
      {props.children}
    </main>
  );
}

/** One sheet over the wall: a bottom sheet on the phone (its grabber, where a drag down goes back), centered on wide
 * forms and TV. `size`: the design's widths (login 860, picker and profile 760, setup 1140). */
export function Sheet(props: { size?: "login" | "narrow" | "setup" | "tv"; label?: string; labelledby?: string; busy?: boolean; onGrab?: () => void; class?: string; children: JSX.Element }) {
  let y0: number | null = null;
  return (
    <section
      class={`sheet ${props.size ?? "narrow"} ${props.class ?? ""}`}
      aria-label={props.label}
      aria-labelledby={props.labelledby}
      aria-busy={props.busy || undefined}
      data-vscroll
    >
      <Show when={props.onGrab && form() === "phone"}>
        <div
          class="sheet-grab"
          aria-hidden="true"
          onPointerDown={(e) => {
            y0 = e.clientY;
            e.currentTarget.setPointerCapture(e.pointerId);
          }}
          onPointerUp={(e) => {
            if (y0 !== null && e.clientY - y0 > 48) props.onGrab?.();
            y0 = null;
          }}
        />
      </Show>
      {props.children}
    </section>
  );
}

/** A sheet's heading row: the title, and Not now or Skip at its right. */
export function SheetHead(props: { id?: string; title: string; children?: JSX.Element }) {
  return (
    <div class="sheet-head">
      <h1 id={props.id} tabindex="-1">
        {props.title}
      </h1>
      {props.children}
    </div>
  );
}
