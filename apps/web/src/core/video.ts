/** What the Versions and player screens both need for one video: the title
 * (name, art, episode names) and the shared Versions view. */

import { createEffect, createMemo } from "solid-js";
import { episodeLabel, parseVideo } from "../lib/format";
import { titleView, versionsView } from "./client";

export function videoContext(kind: string, id: string, videoId: string, bingeGroup?: string) {
  const title = titleView(kind, id);
  const ref = parseVideo(videoId);
  // Keep the title view on this video's season so its episode name is known.
  createEffect(() => {
    const h = title.state.header;
    if (ref && h && h.selectedSeason !== ref.season && h.seasons.includes(ref.season)) {
      void title.command({ type: "selectSeason", season: ref.season });
    }
  });
  const details = () => title.state.header?.details ?? null;
  const versions = createMemo(() => {
    const name = details()?.name;
    if (!name) return null;
    return versionsView({ kind, videoId, title: name, season: ref?.season, episode: ref?.episode, bingeGroup });
  });
  const episode = () => title.state.episodes.find((e) => e.videoId === videoId) ?? null;
  const label = () => (ref ? `${episodeLabel(ref.season, ref.episode)}${episode()?.title ? ` ${episode()?.title}` : ""}` : null);
  return { title, details, versions, episode, ref, label };
}
