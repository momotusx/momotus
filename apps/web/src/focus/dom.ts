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
const content = new Map<string, string>(); // path -> last key outside the rail

const path = () => location.pathname;
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

/** Scrolls the focused element into view inside the scrollers we own:
 * rows scroll sideways with a peek, pages bring the focused row to the top. */
export function reveal(el: HTMLElement) {
  const behavior: ScrollBehavior = reducedMotion() ? "auto" : "smooth";
  const track = el.closest<HTMLElement>("[data-hscroll]");
  if (track && el.offsetParent) {
    const pad = Number(track.dataset.hscroll) || 0;
    const left = el.offsetLeft;
    const right = left + el.offsetWidth;
    let target: number | null = null;
    if (left - pad < track.scrollLeft) target = left - pad;
    else if (right + pad > track.scrollLeft + track.clientWidth) target = right + pad - track.clientWidth;
    if (target !== null) track.scrollTo({ left: Math.max(0, target), behavior });
  }
  const page = el.closest<HTMLElement>("[data-vscroll]");
  if (!page) return;
  if (el.closest("[data-reveal-top]")) {
    if (page.scrollTop > 0) page.scrollTo({ top: 0, behavior });
    return;
  }
  const block = el.closest<HTMLElement>("[data-row]") ?? el;
  const b = block.getBoundingClientRect();
  const p = page.getBoundingClientRect();
  const visible = b.top >= p.top && b.bottom <= p.bottom;
  if (!visible) block.scrollIntoView({ block: block === el ? "nearest" : "start", behavior });
}

export function focusEl(el: HTMLElement) {
  el.focus({ preventScroll: true });
  reveal(el);
}

function initial(): HTMLElement | null {
  const all = candidates();
  return all.find((el) => el.hasAttribute("data-autofocus")) ?? all[0] ?? null;
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
  const list: Candidate<string>[] = els.map((el) => ({
    key: el.dataset.f as string,
    rect: el.getBoundingClientRect(),
    group: groupOf(el),
  }));
  const here = list[els.indexOf(current)] as Candidate<string>;
  let target: string | null = null;
  // Leaving the navigation rail returns to where you were on this screen.
  if (here.group === "rail" && dir === "right") {
    const back = content.get(path());
    if (back && list.some((c) => c.key === back)) target = back;
  }
  target ??= pick(here, list, dir, memory);
  const el = target === null ? null : els[list.findIndex((c) => c.key === target)];
  if (!el) return false;
  focusEl(el);
  return true;
}

/** Makes `key` the member a group restores to (for example the selected tab). */
export function remember(group: string, key: string) {
  memory.set(group, key);
}

let stopRestore: (() => void) | null = null;

export function cancelRestore() {
  stopRestore?.();
  stopRestore = null;
}

/** Focuses what this screen had last (after Back) or its autofocus target,
 * waiting for live data to render it. Any input cancels the wait. */
export function restore(back: boolean) {
  cancelRestore();
  const want = back ? screens.get(path()) : undefined;
  const started = performance.now();
  const attempt = () => {
    const late = performance.now() - started > 4000;
    const el = want && !late ? byKey(want) : initialIfMarked(late);
    if (!el) return false;
    focusEl(el);
    return true;
  };
  if (attempt()) return;
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
  };
}

/** Before data arrives, only an explicit autofocus target counts. */
function initialIfMarked(late: boolean): HTMLElement | null {
  const marked = candidates().find((el) => el.hasAttribute("data-autofocus"));
  return marked ?? (late ? initial() : null);
}

const DIRS: Record<string, Direction> = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right" };
// Tizen and webOS remotes send these for Back.
const BACK_CODES = new Set([10009, 461]);

const typing = (t: EventTarget | null) =>
  t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && t.isContentEditable);

export function installFocus(onBack: () => void) {
  document.addEventListener("focusin", (e) => {
    const el = e.target as HTMLElement;
    const key = el.dataset?.f;
    if (!key) return;
    const group = groupOf(el);
    if (group) memory.set(group, key);
    screens.set(path(), key);
    if (group !== "rail") content.set(path(), key);
  });
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
    if (e.key === "Escape" || e.key === "GoBack" || e.key === "BrowserBack" || BACK_CODES.has(e.keyCode) || (e.key === "Backspace" && !inField)) {
      e.preventDefault();
      cancelRestore();
      onBack();
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
