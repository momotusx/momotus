/** A meta line ("★ 9.2 · 2011–2019 · Drama, Fantasy · 8 seasons") that
 * wraps on a narrow screen must not end a line, or start one, with a dot.
 * Each `b` separator shows only while the items on both sides share its
 * line; the others turn invisible (their space stays, so nothing reflows).
 * Kept up while the line's size or words change. */

import { onCleanup } from "solid-js";

const middle = (el: Element) => {
  const r = el.getBoundingClientRect();
  return (r.top + r.bottom) / 2;
};

function trim(line: HTMLElement) {
  for (const dot of line.querySelectorAll<HTMLElement>(":scope > b")) {
    const before = dot.previousElementSibling;
    const after = dot.nextElementSibling;
    const tol = dot.getBoundingClientRect().height / 2;
    const inLine = !!before && !!after && Math.abs(middle(before) - middle(dot)) < tol && Math.abs(middle(after) - middle(dot)) < tol;
    dot.style.visibility = inLine ? "" : "hidden";
  }
}

/** Use as `ref={separators}` on the line. */
export function separators(line: HTMLElement) {
  const run = () => requestAnimationFrame(() => trim(line));
  const resize = new ResizeObserver(run);
  resize.observe(line);
  const words = new MutationObserver(run);
  words.observe(line, { childList: true, subtree: true, characterData: true });
  void document.fonts?.ready.then(run);
  onCleanup(() => {
    resize.disconnect();
    words.disconnect();
  });
}
