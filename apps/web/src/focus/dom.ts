/** Spatial focus on the DOM.
 *
 * Focusables carry `data-f="<stable key>"`; an ancestor's `data-fgroup` makes
 * them a group (a row) whose last member is remembered. Candidates are read
 * from the DOM on each key press, so there is nothing to register or leak.
 * Focus moves only on input: arrows, Back restoring the previous screen, or
 * the first focus of a screen. */

import { type Candidate, type Direction, pick } from "./spatial";

const SEL = "[data-f]";
const memory = new Map<string, string>(); // group -> key
const screens = new Map<string, string>(); // path -> last key
const spots = new Map<string, { group: string; index: number }>(); // path -> that key's group and place in it
const content = new Map<string, string>(); // path -> last key outside the rail

const path = () => location.pathname;

// The last kind of input: focus may move by itself (autofocus, restore, a
// Skip button taking focus) only on a remote or after keys. On pointer and
// touch builds a ring nobody asked for reads as a glitch.
let lastInput: "keys" | "pointer" | null = null;
// Every key or pointer press, counted: a focus asked for before a press
// never takes over the one the viewer moved since.
let inputs = 0;
export const mayAutoFocus = () => document.documentElement.dataset.form === "tv" || lastInput === "keys";
/** Focuses `el` when focus may move by itself; see `mayAutoFocus`. */
export function autoFocus(el: HTMLElement | null | undefined) {
  if (el && mayAutoFocus()) focusEl(el);
}

/** `autoFocus` once the screen has rendered (a microtask from now), unless
 * the viewer pressed a key or pointed in between: a screen's first focus,
 * or a card's, never takes over the focus the viewer already moved (on a
 * slow core the page settles after the first press). */
export function autoFocusSoon(find: () => HTMLElement | null | undefined) {
  const asked = inputs;
  queueMicrotask(() => {
    if (inputs === asked) autoFocus(find());
  });
}

let recovering = false;
// The element that had the focus last, where it was: when live data takes
// it away, the focus goes to its rebuilt self or its nearest neighbour.
let last: { path: string; key: string; group: string | undefined; x: number; y: number } | null = null;
let backHandler: (() => void) | null = null;
/** The app's one Back: in-app history first, else the screen's parent. */
export const goBack = () => backHandler?.();
const groupOf = (el: Element) => el.closest<HTMLElement>("[data-fgroup]")?.dataset.fgroup;
export const reducedMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

function candidates(): HTMLElement[] {
  const out: HTMLElement[] = [];
  for (const el of document.querySelectorAll<HTMLElement>(SEL)) {
    if (el.closest("[inert]") || (el as HTMLButtonElement).disabled) continue;
    if (el.offsetWidth === 0 && el.offsetHeight === 0) continue;
    out.push(el);
  }
  return out;
}

const byKey = (key: string) => candidates().find((el) => el.dataset.f === key) ?? null;

/** The card a strip of controls is laid over (`data-fowner` names its key). */
const ownerOf = (el: Element) => {
  const key = el.closest<HTMLElement>("[data-fowner]")?.dataset.fowner;
  return key ? byKey(key) : null;
};

/** Right from a card to the first control laid over it; Left back to the card. */
function byOwner(current: HTMLElement, dir: Direction): HTMLElement | null {
  if (dir === "left") return ownerOf(current);
  if (dir !== "right" || current.closest("[data-fowner]")) return null;
  const strip = [...document.querySelectorAll<HTMLElement>("[data-fowner]")].find((s) => s.dataset.fowner === current.dataset.f);
  const first = strip?.querySelector<HTMLElement>("[data-f]");
  return first && first.offsetWidth > 0 ? first : null;
}

/** Scrolls the focused element into view inside the scrollers we own:
 * rows scroll sideways with a peek, pages bring the focused row to the top. */
export function reveal(el: HTMLElement) {
  const behavior: ScrollBehavior = reducedMotion() ? "auto" : "smooth";
  const track = el.closest<HTMLElement>("[data-hscroll]");
  if (track && el.offsetParent) {
    const pad = Number(track.dataset.hscroll) || 0;
    // A lane may reach past its row to give a lifted tile room (its padding), so positions are
    // taken in the lane's content box: where the element sits from the content's start, and the
    // width the content box shows. Rects are scaled with the canvas (desktop), so they unscale.
    const s = getComputedStyle(track);
    const padL = parseFloat(s.paddingLeft) || 0;
    const t = track.getBoundingClientRect();
    const k = t.width / track.offsetWidth || 1;
    const left = (el.getBoundingClientRect().left - t.left) / k - track.clientLeft - padL + track.scrollLeft;
    const right = left + el.offsetWidth;
    const view = track.clientWidth - padL - (parseFloat(s.paddingRight) || 0);
    let target: number | null = null;
    if (left - pad < track.scrollLeft) target = left - pad;
    else if (right + pad > track.scrollLeft + view) target = right + pad - view;
    if (target !== null) track.scrollTo({ left: Math.max(0, target), behavior });
  }
  const page = el.closest<HTMLElement>("[data-vscroll]");
  if (!page) return;
  if (el.closest("[data-reveal-top]")) {
    if (page.scrollTop > 0) page.scrollTo({ top: 0, behavior });
    return;
  }
  // Scroll margins keep a target clear of the floating navigation, the TV
  // safe area and a list's fade, and scrollIntoView honours them. They are
  // canvas pixels while rects are scaled (desktop), so they scale too.
  const p = page.getBoundingClientRect();
  const k = p.height / page.offsetHeight || 1;
  const margins = (x: HTMLElement) => {
    const s = getComputedStyle(x);
    return [(parseFloat(s.scrollMarginTop) || 0) * k, (parseFloat(s.scrollMarginBottom) || 0) * k] as const;
  };
  // A row brings its top into view when it fits; a taller one (a long group
  // of versions) reveals the focused element itself.
  const fits = (x: HTMLElement) => {
    const [a, b] = margins(x);
    return x.getBoundingClientRect().height <= p.height - a - b;
  };
  const row = el.closest<HTMLElement>("[data-row]");
  const block = row && page.contains(row) && fits(row) ? row : el;
  const [above, below] = margins(block);
  const b = block.getBoundingClientRect();
  const visible = b.top >= p.top + above && b.bottom <= p.bottom - below;
  if (!visible) block.scrollIntoView({ block: block === el ? "nearest" : "start", behavior });
}

export function focusEl(el: HTMLElement) {
  el.focus({ preventScroll: true });
  reveal(el);
}

/** Autofocus target, else the first content element, else the rail item of
 * the current section. */
function initial(): HTMLElement | null {
  const all = candidates();
  return (
    all.find((el) => el.hasAttribute("data-autofocus")) ??
    all.find((el) => groupOf(el) !== "rail") ??
    all.find((el) => el.classList.contains("on")) ??
    all[0] ??
    null
  );
}

export function move(dir: Direction): boolean {
  cancelRestore();
  const els = candidates();
  const active = document.activeElement;
  const current = els.find((el) => el === active);
  if (!current) {
    const first = initial();
    if (first) focusEl(first);
    return !!first;
  }
  // Controls laid over their card (an episode's actions) sit inside the
  // card's own box, where spatial search cannot see them: Right from the card
  // enters them, Left leaves them for the card.
  const over = byOwner(current, dir);
  if (over) {
    focusEl(over);
    return true;
  }
  const list: Candidate<string>[] = els.map((el) => ({
    key: el.dataset.f as string,
    rect: el.getBoundingClientRect(),
    group: groupOf(el),
    entry: el.closest<HTMLElement>("[data-fentry]")?.querySelector<HTMLElement>("[data-f]")?.dataset.f,
  }));
  const here = list[els.indexOf(current)] as Candidate<string>;
  let target: string | null = null;
  // Leaving the navigation (a pill along the top) returns to where you were
  // on this screen.
  if (here.group === "rail" && dir === "down") {
    const back = content.get(path());
    if (back && list.some((c) => c.key === back)) target = back;
  }
  target ??= pick(here, list, dir, memory);
  const el = target === null ? null : els[list.findIndex((c) => c.key === target)];
  if (!el) return false;
  focusEl(el);
  return true;
}

/** A profile switch: where focus was belongs to the old profile's screens. */
export function forgetFocus() {
  cancelRestore();
  memory.clear();
  screens.clear();
  spots.clear();
  content.clear();
  last = null;
}

/** Makes `key` the member a group restores to (for example the selected tab). */
export function remember(group: string, key: string) {
  memory.set(group, key);
}

let stopRestore: (() => void) | null = null;
// How long Back waits for the remembered element before taking its place's.
const NEAR_AFTER_MS = 800;

export function cancelRestore() {
  stopRestore?.();
  stopRestore = null;
}

/** Focuses what this screen had last (after Back) or its autofocus target,
 * waiting for live data to render it. A key or pointer press after it was
 * asked for (`asked`, the input count then) ends it: the viewer's own move
 * stands, even when the screen's first focus comes a frame late. */
export function restore(back: boolean, asked = inputs) {
  cancelRestore();
  if (!mayAutoFocus() || inputs !== asked) return;
  const want = back ? screens.get(path()) : undefined;
  const spot = back ? spots.get(path()) : undefined;
  const started = performance.now();
  const attempt = () => {
    if (inputs !== asked) return true;
    const late = performance.now() - started > 4000;
    const el = want && !late ? byKey(want) : initialIfMarked(late);
    if (!el) return false;
    focusEl(el);
    return true;
  };
  if (attempt()) return;
  // The remembered element is gone (a title removed from the list, a row
  // folded): once its group is back without it, the focus goes to the member
  // in its place, near where the viewer was, not to the top of the screen.
  const near = spot
    ? setTimeout(() => {
        if (inputs !== asked || (want && byKey(want))) return;
        const members = candidates().filter((el) => groupOf(el) === spot.group);
        const el = members[Math.min(spot.index, members.length - 1)];
        if (!el) return;
        focusEl(el);
        cancelRestore();
      }, NEAR_AFTER_MS)
    : undefined;
  const observer = new MutationObserver(() => {
    if (attempt()) cancelRestore();
  });
  observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-autofocus"] });
  const timer = setTimeout(() => {
    attempt();
    cancelRestore();
  }, 4100);
  stopRestore = () => {
    observer.disconnect();
    clearTimeout(timer);
    clearTimeout(near);
  };
}

/** Where a lost focus goes: the element with the key that had it (live data
 * rebuilt it: a chip, a subtitle track, a search hit that moved rows),
 * else the nearest member of its group (it left the list), else the
 * screen's first focus. */
function replacement(): HTMLElement | null {
  const was = last;
  if (was && was.path === path()) {
    const all = candidates();
    const same = all.find((el) => el.dataset.f === was.key);
    if (same) return same;
    const near = nearest(
      all.filter((el) => was.group !== undefined && groupOf(el) === was.group),
      was.x,
      was.y,
    );
    if (near) return near;
  }
  return initialIfMarked(true);
}

function nearest(els: HTMLElement[], x: number, y: number): HTMLElement | null {
  let best: HTMLElement | null = null;
  let distance = Infinity;
  for (const el of els) {
    const r = el.getBoundingClientRect();
    const d = Math.hypot(r.left + r.width / 2 - x, r.top + r.height / 2 - y);
    if (d < distance) [best, distance] = [el, d];
  }
  return best;
}

/** Before data arrives, only an explicit autofocus target counts. */
function initialIfMarked(late: boolean): HTMLElement | null {
  const marked = candidates().find((el) => el.hasAttribute("data-autofocus"));
  return marked ?? (late ? initial() : null);
}

const DIRS: Record<string, Direction> = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" };
// Tizen and webOS remotes send these for Back.
const BACK_CODES = new Set([10009, 461]);

/** Back on a keyboard or a remote; Backspace only outside a text field. */
export const isBackKey = (e: KeyboardEvent) =>
  e.key === "Escape" || e.key === "GoBack" || e.key === "BrowserBack" || BACK_CODES.has(e.keyCode) || (e.key === "Backspace" && !typing(e.target));

const typing = (t: EventTarget | null) =>
  t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && t.isContentEditable);

/** `restore` on the next frame (the new screen has rendered by then),
 * unless the viewer pressed something in between. */
export function restoreSoon(back: boolean) {
  const asked = inputs;
  requestAnimationFrame(() => restore(back, asked));
}

export function installFocus(onBack: () => void) {
  backHandler = onBack;
  window.addEventListener(
    "keydown",
    () => {
      lastInput = "keys";
      inputs++;
    },
    { capture: true },
  );
  window.addEventListener(
    "pointerdown",
    () => {
      lastInput = "pointer";
      inputs++;
    },
    { capture: true },
  );
  // Live data can replace the focused element (a chip rebuilt, a row that
  // moved, Play becoming "Add a source" once every addon answered). Focus
  // then falls to the page; on a remote that is a dead end, so it goes to
  // the element with the same key, else its nearest neighbour, else the
  // screen's autofocus target (`replacement`).
  // This stopgap focus is not a choice the viewer made, so it is not
  // remembered: Back still restores what they had.
  new MutationObserver(() => {
    const active = document.activeElement;
    if (!mayAutoFocus() || (active && active !== document.body && active.isConnected)) return;
    const el = replacement();
    if (!el) return;
    recovering = true;
    focusEl(el);
    recovering = false;
  }).observe(document.body, { childList: true, subtree: true });
  document.addEventListener("focusin", (e) => {
    const el = e.target as HTMLElement;
    const key = el.dataset?.f;
    if (!key) return;
    const group = groupOf(el);
    const r = el.getBoundingClientRect();
    last = { path: path(), key, group, x: r.left + r.width / 2, y: r.top + r.height / 2 };
    if (recovering) return;
    if (group) memory.set(group, key);
    screens.set(path(), key);
    if (group) spots.set(path(), { group, index: candidates().filter((c) => groupOf(c) === group).indexOf(el) });
    else spots.delete(path());
    if (group !== "rail") content.set(path(), key);
  });
  // The focus is placed before `reveal` scrolls it into view: once a page or
  // list has scrolled, the point the nearest-member step measures from is
  // where the focused element is now.
  document.addEventListener(
    "scrollend",
    () => {
      const el = document.activeElement as HTMLElement | null;
      if (!last || el?.dataset?.f !== last.key) return;
      const r = el.getBoundingClientRect();
      last = { ...last, x: r.left + r.width / 2, y: r.top + r.height / 2 };
    },
    { capture: true },
  );
  document.addEventListener("pointerdown", cancelRestore, { capture: true });
  window.addEventListener("keydown", (e) => {
    if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return;
    const dir = DIRS[e.key];
    const inField = typing(e.target);
    if (dir) {
      if (inField && (dir === "left" || dir === "right")) return;
      e.preventDefault();
      move(dir);
      return;
    }
    if (isBackKey(e)) {
      e.preventDefault();
      cancelRestore();
      // Back from controls laid over a card returns to the card first.
      const card = e.target instanceof Element ? ownerOf(e.target) : null;
      if (card) focusEl(card);
      else onBack();
      return;
    }
    // Buttons and links activate on Enter natively; other focusables click.
    const t = e.target as HTMLElement;
    if (e.key === "Enter" && t.dataset?.f && !(t instanceof HTMLButtonElement || t instanceof HTMLAnchorElement || inField)) {
      e.preventDefault();
      t.click();
    }
  });
}
