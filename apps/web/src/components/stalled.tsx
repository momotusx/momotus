/** A page still opening (QA r1, Q4 row 36, Q7 row 28): its skeleton first, then one plain line with Try again, at
 * once while the browser is offline and after STALL_MS otherwise, so a page never waits in grey with nothing said.
 * Online again, the line goes and the page goes on opening by itself. */

import RefreshCw from "lucide-solid/icons/refresh-cw";
import { createSignal, type JSX, onCleanup, Show } from "solid-js";
import { online } from "../core/client";

const STALL_MS = 10_000;

export const stalledWords = {
  offline: "You're offline. This page opens when the network is back.",
  slow: "This is taking longer than it should.",
};

export function Stalled(props: { children: JSX.Element; retry: () => void; fkey: string }) {
  const [late, setLate] = createSignal(false);
  const timer = setTimeout(() => setLate(true), STALL_MS);
  onCleanup(() => clearTimeout(timer));
  const line = () => (!online() ? stalledWords.offline : late() ? stalledWords.slow : null);
  return (
    <Show when={line()} fallback={props.children}>
      {(text) => (
        <section class="problem stalled" role="status">
          <p>{text()}</p>
          <button class="btn primary" data-f={props.fkey} data-autofocus onClick={() => props.retry()}>
            <RefreshCw aria-hidden />
            Try again
          </button>
        </section>
      )}
    </Show>
  );
}
