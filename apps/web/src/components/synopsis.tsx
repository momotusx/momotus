/** A title's full synopsis over the page (plans/design-v3-phase34-web.md, the info column): a solid card,
 * never text opening in place, which would climb the hero's column past the ramps the legibility check
 * measures. Modal; Back closes it through the one Back stack, and the browser's Back too (its own history
 * entry). On TV the text splits into focusable blocks (lib/blocks.ts), so the remote scrolls it by
 * moving focus, a long single paragraph included. */

import X from "lucide-solid/icons/x";
import { For, onCleanup, onMount } from "solid-js";
import { pushBack } from "../focus/back";
import { focusEl } from "../focus/dom";
import { form } from "../form";
import { synopsisBlocks } from "../lib/blocks";

export function SynopsisDialog(props: { name: string; text: string; onClose: () => void }) {
  const paragraphs = () => (form() === "tv" ? synopsisBlocks(props.text) : props.text.split(/\n{2,}/).map((p) => p.trim()).filter(Boolean));
  onMount(() => {
    onCleanup(pushBack(() => props.onClose(), { entry: true }));
    queueMicrotask(() => {
      const close = document.querySelector<HTMLElement>('[data-f="synopsis:close"]');
      if (close) focusEl(close);
    });
  });
  return (
    <div class="syn-layer" role="dialog" aria-modal="true" aria-labelledby="syn-title">
      <div class="syn-scrim" aria-hidden="true" onClick={() => props.onClose()} />
      <section class="syn-card" data-vscroll data-fgroup="synopsis">
        <header class="syn-head">
          <h2 id="syn-title">{props.name}</h2>
          <button class="rbtn" data-f="synopsis:close" aria-label="Close" onClick={() => props.onClose()}>
            <span class="ring">
              <X aria-hidden />
            </span>
          </button>
        </header>
        <For each={paragraphs()}>
          {(p, i) => (
            <p class="syn-text" data-f={`synopsis:${i()}`} tabindex="-1">
              {p}
            </p>
          )}
        </For>
      </section>
    </div>
  );
}
