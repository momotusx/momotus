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
  const episode = () => title.state.episodes.find((e) => e.videoId === videoId) ?? null;
  const versions = createMemo(() => {
    const d = details();
    if (!d?.name) return null;
    // What the title and episode say now; the view opens once per video, so
    // facts that arrive later are the core's to read from the meta.
    const season = ref ? title.state.episodes.filter((e) => e.season === ref.season).length : 0;
    return versionsView({
      kind,
      videoId,
      title: d.name,
      season: ref?.season,
      episode: ref?.episode,
      bingeGroup,
      runtime: d.runtime ?? undefined,
      releaseInfo: d.releaseInfo ?? undefined,
      released: episode()?.released ?? undefined,
      episodesInSeason: season || undefined,
    });
  });
  const label = () => (ref ? `${episodeLabel(ref.season, ref.episode)}${episode()?.title ? ` ${episode()?.title}` : ""}` : null);
  return { title, details, versions, episode, ref, label };
}
