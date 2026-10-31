/** A reason in plain words (lib/words.ts problemWords), the core's own text
 * behind Details for support. A view that could not open passes its words as
 * `reason` and the core's text as `raw` (`ViewStatus`). */

import { Show } from "solid-js";
import { problemWords } from "../lib/words";

export function Why(props: { reason: string; raw?: string; class?: string }) {
  const w = () => (props.raw === undefined ? problemWords(props.reason) : { text: props.reason, raw: props.raw });
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
