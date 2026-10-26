/** Artwork that degrades to words: backdrops fade in once decoded, logos fall
 * back to the title set in the display face, posters to their name. */

import { createEffect, createSignal, on, Show } from "solid-js";
import { form } from "../form";
import { type TrimmedLogo, trimmedLogo } from "../lib/trim";
import { pageColor } from "../theme/page";
import { type Ramp, rampCss, ramps } from "./ramps";

/** The backdrop, else (none named, or it fails to load) `fallback`, shown
 * blurred, under its `ramp`; `onMissing` says the real backdrop is not there. */
export function Backdrop(props: {
  src: string | null | undefined;
  class?: string;
  dim?: boolean;
  ramp?: Ramp;
  fallback?: string | null;
  onMissing?: () => void;
}) {
  const [failed, setFailed] = createSignal(false);
  createEffect(on(() => props.src, () => setFailed(false), { defer: true }));
  const shown = () => (props.src && !failed() ? props.src : (props.fallback ?? null));
  const standIn = () => !!shown() && shown() !== props.src;
  createEffect(() => {
    if (!props.src || failed()) props.onMissing?.();
  });
  return (
    <div class={`bd ${props.class ?? ""}`} classList={{ dim: props.dim, blur: standIn() }} aria-hidden="true">
      <Show when={shown()} keyed>
        {(src) => (
          <img
            src={src}
            alt=""
            decoding="async"
            class="fade"
            onLoad={(e) => e.currentTarget.classList.add("in")}
            onError={() => src === props.src && setFailed(true)}
          />
        )}
      </Show>
      <Show when={props.ramp}>{(r) => <div class="ramp" style={{ background: rampCss(ramps[r()], pageColor(form())) }} />}</Show>
    </div>
  );
}

/** A logo with its transparent margins cut away (lib/trim.ts): undefined
 * while that runs (the box is there, nothing jumps when the cut lands), null
 * when the logo does not load at all. */
export function useTrimmedLogo(src: () => string | null | undefined): () => TrimmedLogo | null | undefined {
  const [out, setOut] = createSignal<TrimmedLogo | null | undefined>(undefined);
  createEffect(
    on(src, (url) => {
      setOut(undefined);
      if (url) void trimmedLogo(url).then((t) => src() === url && setOut(t));
    }),
  );
  return out;
}

export function Logo(props: { src: string | null | undefined; name: string; class?: string; level?: 1 | 2 }) {
  const [broken, setBroken] = createSignal(false);
  const trimmed = useTrimmedLogo(() => props.src);
  const ratio = () => trimmed()?.ratio;
  return (
    <Show
      when={props.src && !broken() && trimmed() !== null}
      fallback={props.level === 2 ? <h2 class={`logo-text ${props.class ?? ""}`}>{props.name}</h2> : <h1 class={`logo-text ${props.class ?? ""}`}>{props.name}</h1>}
    >
      {/* The page keeps its heading for screen readers when the name is a picture. */}
      {props.level === 2 ? <h2 class="sr">{props.name}</h2> : <h1 class="sr">{props.name}</h1>}
      {/* While the cut runs the box holds its place, hidden: an image with no
          source would show its alt text and a broken-image mark. */}
      <img
        class={`logo ${props.class ?? ""}`}
        classList={{ pending: trimmed() === undefined }}
        src={trimmed()?.src}
        alt=""
        decoding="async"
        // Its shape, known with its source, so the CSS sizes every logo to one
        // area (a stacked logo as tall as it needs, a wide one as low) from the
        // first frame it is drawn in.
        style={{ "--r": ratio()?.toFixed(3) }}
        // Narrower than 3.6:1, Home's hero draws it taller than 100 px, so
        // the synopsis under it gives a line back (home.css).
        data-shape={ratio() === undefined ? undefined : (ratio() ?? 0) < 3.6 ? "tall" : "wide"}
        onError={() => setBroken(true)}
      />
    </Show>
  );
}

/** A poster over a name card: the title's name sits centered in the surface
 * until the picture has decoded, and stays when there is none, so a tile is
 * never a blank box. A 16:9 still that fails to load gives way to `fallback` (its poster);
 * `top` crops a poster in a still's place from its upper part. */
export function Poster(props: {
  src: string | null;
  fallback?: string | null;
  name: string;
  w: number;
  h: number;
  eager?: boolean;
  nameCard?: boolean;
  top?: boolean;
}) {
  const [failed, setFailed] = createSignal(0);
  // A new picture starts again from its own source, as Backdrop does.
  createEffect(on(() => props.src, () => setFailed(0), { defer: true }));
  const src = () => [props.src, props.fallback].filter((s): s is string => !!s)[failed()];
  return (
    <>
      <Show when={props.nameCard !== false}>
        <span class="pic-name">{props.name}</span>
      </Show>
      <Show when={src()} keyed>
        {(url) => (
          <img
            src={url}
            width={props.w}
            height={props.h}
            alt=""
            loading={props.eager ? "eager" : "lazy"}
            decoding="async"
            class="fade"
            classList={{ top: props.top || url !== props.src }}
            onLoad={(e) => e.currentTarget.classList.add("in")}
            onError={() => setFailed((n) => n + 1)}
          />
        )}
      </Show>
    </>
  );
}
