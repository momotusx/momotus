/** Artwork that degrades to words: backdrops fade in once decoded, logos fall
 * back to the title set in the display face, posters to their name. */

import { createSignal, Show } from "solid-js";

export function Backdrop(props: { src: string | null | undefined; class?: string; dim?: boolean }) {
  return (
    <div class={`bd ${props.class ?? ""}`} classList={{ dim: props.dim }} aria-hidden="true">
      <Show when={props.src} keyed>
        {(src) => (
          <img src={src} alt="" decoding="async" class="fade" onLoad={(e) => e.currentTarget.classList.add("in")} />
        )}
      </Show>
    </div>
  );
}

export function Logo(props: { src: string | null | undefined; name: string; class?: string; level?: 1 | 2 }) {
  const [broken, setBroken] = createSignal(false);
  return (
    <Show
      when={props.src && !broken()}
      fallback={props.level === 2 ? <h2 class={`logo-text ${props.class ?? ""}`}>{props.name}</h2> : <h1 class={`logo-text ${props.class ?? ""}`}>{props.name}</h1>}
    >
      <img
        class={`logo ${props.class ?? ""}`}
        src={props.src as string}
        alt={props.name}
        decoding="async"
        onError={() => setBroken(true)}
      />
    </Show>
  );
}

/** A poster image with a name card while it loads or when it is missing. */
export function Poster(props: { src: string | null; name: string; w: number; h: number }) {
  const [broken, setBroken] = createSignal(false);
  return (
    <Show when={props.src && !broken()} fallback={<span class="pic-name">{props.name}</span>}>
      <img
        src={props.src as string}
        width={props.w}
        height={props.h}
        alt=""
        loading="lazy"
        decoding="async"
        class="fade"
        onLoad={(e) => e.currentTarget.classList.add("in")}
        onError={() => setBroken(true)}
      />
    </Show>
  );
}
