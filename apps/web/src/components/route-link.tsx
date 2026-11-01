/** A link to a page that names a title (plans/batch-10-incognito.md 5.5). Outside Incognito it is the router's `A`. In
 * Incognito the address it shows is "/" and its route rides in `data-route` (src/incognito.ts puts the route in place
 * for the click), so a middle click, "Open in new tab" or a drag opens "/", which lands on the chooser. It is a plain
 * anchor there: the router's `A` would match "/" against Home and mark every such link as the current page. */

import { A } from "@solidjs/router";
import { type ComponentProps, Show, splitProps } from "solid-js";
import { incognito } from "../core/profiles";

export function RouteA(props: ComponentProps<typeof A>) {
  const [own, rest] = splitProps(props, ["href", "state", "class", "classList", "end", "activeClass", "inactiveClass"]);
  return (
    <Show when={incognito()} fallback={<A {...props} />}>
      <a {...rest} link href="/" data-route={own.href} state={own.state === undefined ? undefined : JSON.stringify(own.state)} class={own.class} classList={own.classList} />
    </Show>
  );
}
