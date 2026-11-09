/** The player's Trakt mark (`PlaybackSnapshot.trakt`): whether this playback
 * reaches Trakt. Tracked, it is a small pill with the whole sentence for
 * screen readers; not tracked, the sentence itself shows, on every form, so
 * a TV or a phone says why too, but only while `notice` is on (once per
 * play, a few seconds: `noticeShows`). Nothing when the profile does not use
 * Trakt. */

import CircleAlert from "lucide-solid/icons/circle-alert";
import Radio from "lucide-solid/icons/radio";
import { Show } from "solid-js";
import type { TraktPlayback } from "../core/generated/TraktPlayback";
import { playbackMark, traktWords } from "../lib/trakt";
import "./trakt-mark.css";

export function TraktMark(props: { state: TraktPlayback | undefined; notice: boolean }) {
  const mark = () => {
    const m = playbackMark(props.state);
    return m?.tone === "warn" && !props.notice ? null : m;
  };
  return (
    <Show when={mark()}>
      {(m) => (
        <p class={`trakt-mark ${m().tone}`} data-trakt={props.state} role={m().tone === "warn" ? "status" : undefined}>
          <Show
            when={m().tone === "on"}
            fallback={
              <>
                <CircleAlert aria-hidden />
                <span>{m().label}</span>
              </>
            }
          >
            <Radio aria-hidden />
            <span aria-hidden="true">{traktWords.title}</span>
            <span class="sr">{m().label}</span>
          </Show>
        </p>
      )}
    </Show>
  );
}
