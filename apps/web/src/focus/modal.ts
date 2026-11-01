import { onCleanup, onMount } from "solid-js";
import { screenChanged } from "./ghost-tap";

/** While a modal is open, everything outside it is inert (no focus, no clicks, hidden from
 * screen readers); focus goes back where it was when it closes. Without `inside`, the modal is
 * the page's main and only the navigation goes inert. */
export function holdModal(inside?: () => HTMLElement) {
  const before = document.activeElement as HTMLElement | null;
  // The opener is often rebuilt when the dialog closes: it is found again by its key.
  const key = before?.dataset.f;
  const held: HTMLElement[] = [];
  onMount(() => {
    const keep = inside?.();
    const outside = keep ? siblingsUpTo(keep) : [...document.querySelectorAll<HTMLElement>(".app > nav")];
    for (const el of outside) {
      if (el.inert) continue;
      el.inert = true;
      held.push(el);
    }
  });
  onCleanup(() => {
    screenChanged();
    for (const el of held) el.inert = false;
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
