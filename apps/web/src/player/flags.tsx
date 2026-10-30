/** The subtitle picker's language flags: HatScripts circle-flags 2.8.3 (MIT,
 * flags/LICENSE.md), its language set, bundled as SVG files and drawn as
 * images, never emoji (design/DECISION.md). A flag stands for a language by
 * circle-flags' own convention; the name always sits beside it. */

import Languages from "lucide-solid/icons/languages";
import { createSignal, Show } from "solid-js";
import { flagFile, flagGlyph } from "../lib/subtitles";

// Each flag its own file, fetched when a row shows it, not inlined in the script.
const files = import.meta.glob<string>("./flags/*.svg", { eager: true, query: "?url&no-inline", import: "default" });
const path = (name: string) => `./flags/${name}.svg`;

/** A language's flag in its circle; with none, or one that did not load, a neutral round mark with the language
 * glyph or its two letters. */
export function Flag(props: { language: string; lazy?: boolean }) {
  const [failed, setFailed] = createSignal<string | null>(null);
  const src = () => {
    const name = flagFile(props.language, (n) => path(n) in files);
    const url = name ? files[path(name)] : undefined;
    return url === failed() ? undefined : url;
  };
  // The image's attributes reserve a square (CSS sets the size); a long list loads its flags as they scroll in.
  return (
    <Show
      when={src()}
      fallback={
        <span class="flag none" aria-hidden="true">
          <Show when={flagGlyph(props.language)} fallback={props.language.slice(0, 2).toUpperCase()}>
            <Languages class="flag-glyph" />
          </Show>
        </span>
      }
    >
      {(s) => <img class="flag" src={s()} alt="" width="64" height="64" decoding="async" loading={props.lazy ? "lazy" : "eager"} onError={() => setFailed(s())} />}
    </Show>
  );
}
