/** A page fixed to the address it opened with: `address` keys it, so a new
 * address is a new page, and `read` runs once per page, untracked. Live props
 * would let the closing page see the next address: a cleanup that reads a
 * memo then recomputes it halfway through disposal, Solid throws inside the
 * router's transition, and the old page stays on screen under the new address. */

import { type Component, createComponent, Show, untrack } from "solid-js";

export function KeyedPage<P extends object>(props: { address: string; read: () => P; page: Component<P> }) {
  return (
    <Show when={props.address} keyed>
      {(_) => createComponent(props.page, untrack(props.read))}
    </Show>
  );
}
