/** A title's trailer over the page: the YouTube privacy-enhanced player,
 * with focus kept on our own buttons so a remote's Back always reaches us.
 * Back or Escape closes it. */

import X from "lucide-solid/icons/x";
import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Trailer } from "../core/generated/Trailer";
import { focusEl } from "../focus/dom";

const BACK = new Set(["Escape", "Backspace", "GoBack", "BrowserBack"]);

export function TrailerOverlay(props: { trailers: Trailer[]; onClose: () => void }) {
  const [index, setIndex] = createSignal(0);
  const current = () => props.trailers[index()] ?? props.trailers[0];
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (BACK.has(e.key) || e.keyCode === 10009 || e.keyCode === 461) {
        e.preventDefault();
        e.stopImmediatePropagation();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    queueMicrotask(() => {
      const close = document.querySelector<HTMLElement>('[data-f="trailer:close"]');
      if (close) focusEl(close);
    });
    onCleanup(() => window.removeEventListener("keydown", onKey, { capture: true }));
  });
  return (
    <div class="trailer" role="dialog" aria-modal="true" aria-label={`Trailer: ${current()?.title ?? ""}`}>
      <div class="trailer-frame">
        <Show when={current()} keyed>
          {(t) => (
            <iframe
              src={`https://www.youtube-nocookie.com/embed/${encodeURIComponent(t.youtubeId)}?autoplay=1&rel=0&playsinline=1`}
              title={`Trailer: ${t.title}`}
              allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
              referrerpolicy="strict-origin-when-cross-origin"
              tabindex="-1"
            />
          )}
        </Show>
      </div>
      <div class="trailer-bar" data-fgroup="trailer">
        <p>
          <span class="kicker">Trailer</span>
          {current()?.title}
        </p>
        <Show when={props.trailers.length > 1}>
          <div class="trailer-picks" role="radiogroup" aria-label="Trailers">
            <For each={props.trailers}>
              {(_, i) => (
                <button class="chip" classList={{ on: i() === index() }} role="radio" aria-checked={i() === index()} data-f={`trailer:${i()}`} onClick={() => setIndex(i())}>
                  {i() === 0 ? "Main trailer" : `Trailer ${i() + 1}`}
                </button>
              )}
            </For>
          </div>
        </Show>
        <button class="btn" data-f="trailer:close" onClick={props.onClose}>
          <X aria-hidden />
          Close
        </button>
      </div>
    </div>
  );
}
