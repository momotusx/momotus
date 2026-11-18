import { onCleanup, onMount } from "solid-js";
import { screenChanged } from "./ghost-tap";

/** Native `inert` (Chrome 102). Below it (LG webOS 6 and 22) the property is a plain field, so `hold` does inert's work:
 * the attribute (the D-pad engine skips `[inert]`), hidden from screen readers, no pointer, and a focus guard. */
const nativeInert = typeof HTMLElement !== "undefined" && "inert" in HTMLElement.prototype;

const isInert = (el: HTMLElement) => (nativeInert ? el.inert : el.hasAttribute("inert"));

/** Makes `el` inert; returns what undoes it. */
function hold(el: HTMLElement): () => void {
  if (nativeInert) {
    el.inert = true;
    return () => (el.inert = false);
  }
  const hidden = el.getAttribute("aria-hidden");
  const pointer = el.style.pointerEvents;
  el.setAttribute("inert", "");
  el.setAttribute("aria-hidden", "true");
  el.style.pointerEvents = "none";
  return () => {
    el.removeAttribute("inert");
    if (hidden === null) el.removeAttribute("aria-hidden");
    else el.setAttribute("aria-hidden", hidden);
    el.style.pointerEvents = pointer;
  };
}

/** While a modal is open, everything outside it is inert (no focus, no clicks, hidden from
 * screen readers); focus goes back where it was when it closes. Without `inside`, the modal is
 * the page's main and only the navigation goes inert. What matches `spare` stays live (the desktop profile panel
 * keeps the header, whose avatar closes it, and Incognito's ends-soon warning). */
export function holdModal(inside?: () => HTMLElement, spare?: string) {
  const before = document.activeElement as HTMLElement | null;
  // The opener is often rebuilt when the dialog closes: it is found again by its key.
  const key = before?.dataset.f;
  const held: HTMLElement[] = [];
  const undo: (() => void)[] = [];
  // Without native inert, Tab and a script's focus() still reach a held element: focus goes back into the modal.
  let lastInside: HTMLElement | null = null;
  const guard = (e: FocusEvent) => {
    const to = e.target as HTMLElement;
    if (!held.some((el) => el.contains(to))) return void (lastInside = to);
    const back = lastInside?.isConnected ? lastInside : (inside?.().querySelector<HTMLElement>("[data-f], button, a[href], input, [tabindex]") ?? null);
    if (back) back.focus();
    else to.blur();
  };
  onMount(() => {
    const keep = inside?.();
    const outside = keep ? siblingsUpTo(keep) : [...document.querySelectorAll<HTMLElement>(".app > nav")];
    for (const el of outside) {
      if (isInert(el) || (spare && el.matches(spare))) continue;
      undo.push(hold(el));
      held.push(el);
    }
    if (!nativeInert) document.addEventListener("focusin", guard, true);
  });
  onCleanup(() => {
    screenChanged();
    if (!nativeInert) document.removeEventListener("focusin", guard, true);
    for (const u of undo) u();
    queueMicrotask(() => {
      const back = before?.isConnected ? before : key ? document.querySelector<HTMLElement>(`[data-f="${key}"]`) : null;
      back?.focus();
    });
  });
}

/** Every element beside `el` and beside each of its ancestors, up to the body. */
function siblingsUpTo(el: HTMLElement): HTMLElement[] {
  const out: HTMLElement[] = [];
  for (let node: HTMLElement | null = el; node && node !== document.body; node = node.parentElement) {
    for (const sib of node.parentElement?.children ?? []) if (sib !== node && sib instanceof HTMLElement) out.push(sib);
  }
  return out;
}
