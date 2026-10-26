/** The player's loading layer (plans/player-polish.md 1.2, design/DECISION.md
 * "Loading"), as Stremio draws it: until the first frame, the title's
 * backdrop dimmed and blurred over the page color; on a later stall, the
 * paused frame. Over it the title's logo, trimmed to its drawn box, as a
 * faint copy with the solid copy revealed from the left by the fill (its
 * edge feathered), or the title's name filled the same way; under it the
 * player's and the core's words, small and quiet. */

import { createMemo, For, type JSX, Show } from "solid-js";
import { useTrimmedLogo } from "../components/art";

export type Wait = "start" | "stall";

export function Loading(props: {
  wait: Wait | null;
  /** 0 to 1; the layer never draws it lower within one wait. */
  fill: number;
  logo: string | null | undefined;
  name: string | undefined;
  background: string | null | undefined;
  lines: string[];
  /** Under the stage line, in the layer's own layout: the player's "Choose another version". */
  action?: JSX.Element;
}) {
  const logo = useTrimmedLogo(() => props.logo);
  // One count per wait: each draws its own mark, so a stall never runs its
  // fill back from where the start left it.
  const wait = createMemo<{ wait: Wait | null; n: number }>((prev) => ({ wait: props.wait, n: props.wait && !prev.wait ? prev.n + 1 : prev.n }), { wait: null, n: 0 });
  const fill = createMemo<{ n: number; at: number }>(
    (prev) => {
      const n = wait().n;
      if (!props.wait) return prev;
      return { n, at: prev.n === n ? Math.max(prev.at, props.fill) : props.fill };
    },
    { n: 0, at: 0 },
  );
  // A logo URL is cut first (undefined meanwhile): nothing shows until then, the name only when there is none.
  const asName = () => !props.logo || logo() === null;

  return (
    <>
      {/* The picture parts are hidden from screen readers (the status line below speaks the words); the action is not.
          While it waits nothing takes the focus by itself (`data-focus-wait`, focus/dom.ts): its action only on a key. */}
      <div class="ld" classList={{ on: !!props.wait, stall: props.wait === "stall" }} data-focus-wait={props.wait ? "" : undefined}>
        <Show when={props.background}>
          {(src) => <img class="ld-bd" src={src()} alt="" aria-hidden="true" decoding="async" onLoad={(e) => e.currentTarget.classList.add("in")} />}
        </Show>
        <div class="ld-body">
          <Show when={wait().n} keyed>
            {(n) => (
              // The logo's shape sizes it (a wide logo by the width, a stacked one by its height), known with its source.
              <div class="ld-mark" data-wait={n} style={{ "--p": String(fill().at), "--r": logo()?.ratio.toFixed(3) }} aria-hidden="true">
                <Show
                  when={!asName() && logo()?.src}
                  fallback={
                    <Show when={asName() && props.name}>
                      {(name) => (
                        <>
                          <span class="ld-name ld-faint" aria-hidden="true">
                            {name()}
                          </span>
                          <span class="ld-name ld-solid">{name()}</span>
                        </>
                      )}
                    </Show>
                  }
                >
                  {(src) => (
                    <>
                      <img
                        class="ld-logo ld-faint"
                        src={src()}
                        alt=""
                        decoding="async"
                      />
                      <img class="ld-logo ld-solid" src={src()} alt={props.name ?? ""} decoding="async" />
                    </>
                  )}
                </Show>
              </div>
            )}
          </Show>
          <p class="ld-cap" aria-hidden="true">
            <For each={props.lines}>{(l) => <span>{l}</span>}</For>
          </p>
          <Show when={props.action}>
            <div class="ld-act">{props.action}</div>
          </Show>
        </div>
      </div>
      {/* Read out from a region that is always there: one inside the hidden layer may miss its first words. */}
      <p class="sr" role="status">
        {props.wait ? props.lines.join(". ") : ""}
      </p>
    </>
  );
}
