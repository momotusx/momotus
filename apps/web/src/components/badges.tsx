/** The title's quality badges (plans/appletv-polish.md decision 9, 4.4): the release Play starts, which
 * is Resume's when resuming (C33), as Apple's badge row draws them, in words, never a logo. */
import { For, Show } from "solid-js";
import type { Captions } from "../core/generated/Captions";
import type { VersionRow } from "../core/generated/VersionRow";
import "./badges.css";

export type Badge = { text: string; look: "filled" | "outlined"; spoken: string };

/** One badge per family, in Apple's order: resolution, HDR, audio, then CC, SDH and AD. */
export function qualityBadges(release: Pick<VersionRow, "quality" | "features"> | null | undefined, captions: Captions | null | undefined): Badge[] {
  const out: Badge[] = [];
  const has = (f: string) => !!release?.features.includes(f);
  if (release?.quality === "4K") out.push({ text: "4K", look: "filled", spoken: "4K" });
  else if (release && ["1080p", "1440p", "720p"].includes(release.quality)) out.push({ text: "HD", look: "filled", spoken: "HD" });
  const hdr = ["Dolby Vision", "HDR10+", "HDR10", "HLG"].find(has);
  if (hdr) out.push({ text: hdr, look: "outlined", spoken: hdr === "HLG" ? "HDR" : hdr });
  if (has("Atmos")) out.push({ text: "Dolby Atmos", look: "outlined", spoken: "Dolby Atmos" });
  else {
    const channels = ["7.1", "5.1"].find(has);
    if (channels) out.push({ text: channels, look: "outlined", spoken: `${channels} surround` });
  }
  if (captions && captions.languages > 0) out.push({ text: "CC", look: "outlined", spoken: "Subtitles available" });
  if (captions?.hearingImpaired) out.push({ text: "SDH", look: "outlined", spoken: "Subtitles for the deaf and hard of hearing" });
  if (has("AD")) out.push({ text: "AD", look: "outlined", spoken: "Audio description" });
  return out;
}

/** The row, spoken once as what the viewer gets: "Plays in 4K, Dolby Vision, Dolby Atmos". */
export function QualityBadges(props: { badges: Badge[] }) {
  return (
    <Show when={props.badges.length}>
      <span class="qbadges" role="img" aria-label={`Plays in ${props.badges.map((b) => b.spoken).join(", ")}`}>
        <For each={props.badges}>{(b) => <span class={`qbadge ${b.look}`}>{b.text}</span>}</For>
      </span>
    </Show>
  );
}
