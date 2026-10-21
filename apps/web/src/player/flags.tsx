/** The subtitle picker's language flags: HatScripts circle-flags 2.8.3 (MIT,
 * flags/LICENSE.md), its language set, bundled as SVG files and drawn as
 * images, never emoji (design/DECISION.md). A flag stands for a language by
 * circle-flags' own convention; the name always sits beside it. */

import { Show } from "solid-js";
import { flagFile } from "../lib/subtitles";

// Each flag its own file, fetched when a row shows it, not inlined in the script.
const files = import.meta.glob<string>("./flags/*.svg", { eager: true, query: "?url&no-inline", import: "default" });
const path = (name: string) => `./flags/${name}.svg`;

/** A language's flag in its circle; with none, a neutral round mark with its two letters. */
export function Flag(props: { language: string }) {
  const src = () => {
    const name = flagFile(props.language, (n) => path(n) in files);
    return name ? files[path(name)] : undefined;
  };
  return (
    <Show when={src()} fallback={<span class="flag none" aria-hidden="true">{props.language.slice(0, 2).toUpperCase()}</span>}>
      {(s) => <img class="flag" src={s()} alt="" decoding="async" />}
    </Show>
  );
}
