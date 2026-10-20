/** A reason in plain words (lib/words.ts problemWords), the core's own text
 * behind Details for support. */

import { Show } from "solid-js";
import { problemWords } from "../lib/words";

export function Why(props: { reason: string; class?: string }) {
  const w = () => problemWords(props.reason);
  return (
    <>
      <p class={props.class}>{w().text}</p>
      <Show when={w().raw}>
        {(raw) => (
          <details class="why-raw">
            <summary>Details</summary>
            <code>{raw()}</code>
          </details>
        )}
      </Show>
    </>
  );
}
