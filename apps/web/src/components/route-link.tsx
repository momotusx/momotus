/** A link to a page that names a title (plans/batch-10-incognito.md 5.5). It is a plain anchor the router's own click
 * handler follows, as its `A` renders one, without `A`'s per-link memos: a page of tiles and search hits holds hundreds
 * of these, and each `A` resolved its path and matched it against the address on every navigation, for an
 * active class nothing here reads. In Incognito the address it shows is "/" and its route rides in `data-route`
 * (src/incognito.ts puts the route in place for the click), so a middle click, "Open in new tab" or a drag opens "/",
 * which lands on the chooser. */

import type { A } from "@solidjs/router";
import { type ComponentProps, Show, splitProps } from "solid-js";
import { incognito } from "../core/profiles";

export function RouteA(props: ComponentProps<typeof A>) {
  const [own, rest] = splitProps(props, ["href", "state", "class", "classList", "end", "activeClass", "inactiveClass"]);
  const state = () => (own.state === undefined ? undefined : JSON.stringify(own.state));
  return (
    <Show when={incognito()} fallback={<a {...rest} link href={own.href} state={state()} class={own.class} classList={own.classList} />}>
      <a {...rest} link href="/" data-route={own.href} state={state()} class={own.class} classList={own.classList} />
    </Show>
  );
}
