/** The player's Trakt mark (`PlaybackSnapshot.trakt`): whether this playback
 * reaches Trakt, as a small pill with its icon, and the whole sentence for
 * screen readers. Nothing when the profile does not use Trakt. */

import CircleAlert from "lucide-solid/icons/circle-alert";
import Radio from "lucide-solid/icons/radio";
import { Show } from "solid-js";
import type { TraktPlayback } from "../core/generated/TraktPlayback";
import { playbackMark, traktWords } from "../lib/trakt";
import "./trakt-mark.css";

export function TraktMark(props: { state: TraktPlayback | undefined }) {
  return (
    <Show when={playbackMark(props.state)}>
      {(m) => (
        <p class={`trakt-mark ${m().tone}`} data-trakt={props.state} title={m().label}>
          <Show when={m().tone === "on"} fallback={<CircleAlert aria-hidden />}>
            <Radio aria-hidden />
          </Show>
          <span aria-hidden="true">{traktWords.title}</span>
          <span class="sr">{m().label}</span>
        </p>
      )}
    </Show>
  );
}
