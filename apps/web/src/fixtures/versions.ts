/** Sample data for the Versions screen until the core's versions view lands.
 * The types mirror the planned Rust shape (serde camelCase, enums tagged on
 * `state` like RowState), so live data replaces `sampleVersions` and nothing
 * else changes. */

import { parseVideo } from "../lib/format";

export type VersionStatus = { state: "ready" } | { state: "torrent"; seeds: number } | { state: "needsDownload" };

export type VersionGroup = "bestQuality" | "smallerFaster" | "otherLanguages" | "needsDownload";

export interface VersionRow {
  key: string;
  /** Resolution word: "4K", "1080p", "720p". */
  quality: string;
  /** Words, never codes: "Dolby Vision", "HDR10", "Atmos", "5.1". */
  features: string[];
  sizeBytes: number;
  status: VersionStatus;
  /** Debrid service or addon that delivers it. */
  via: string;
  releaseName: string;
  /** "EN audio", "TR subs". */
  languages: string[];
  /** Up to three plain reasons, shown for the recommended row. */
  reasons: string[];
  group: VersionGroup;
  /** Other sources of the same release tried in order if this one fails. */
  failoverCount: number;
}

export interface VersionsSnapshot {
  /** What this device plays, in words. */
  device: string;
  recommended: VersionRow | null;
  rows: VersionRow[];
  hidden: { count: number; summary: string };
}

export const groupLabels: Record<VersionGroup, string> = {
  bestQuality: "Best quality",
  smallerFaster: "Smaller and faster",
  otherLanguages: "Other languages",
  needsDownload: "Needs download",
};

export const groupOrder: VersionGroup[] = ["bestQuality", "smallerFaster", "otherLanguages", "needsDownload"];

const GB = 1e9;

/** Builds believable release names for whichever title is open. */
export function sampleVersions(titleName: string, videoId: string): VersionsSnapshot {
  const ref = parseVideo(videoId);
  const slug = titleName.replace(/[^A-Za-z0-9]+/g, ".").replace(/^\.|\.$/g, "") || "Title";
  const tag = ref ? `.S${String(ref.season).padStart(2, "0")}E${String(ref.episode).padStart(2, "0")}` : "";
  const n = (rest: string) => `${slug}${tag}.${rest}`;
  let i = 0;
  const row = (
    group: VersionGroup,
    quality: string,
    features: string[],
    size: number,
    status: VersionStatus,
    via: string,
    release: string,
    languages: string[],
    failoverCount = 0,
    reasons: string[] = [],
  ): VersionRow => ({
    key: `v${++i}`,
    quality,
    features,
    sizeBytes: Math.round(size * GB),
    status,
    via,
    releaseName: n(release),
    languages,
    reasons,
    group,
    failoverCount,
  });
  const ready = { state: "ready" } as const;
  return {
    device: "This TV: 4K, Dolby Vision, Atmos",
    recommended: row(
      "bestQuality",
      "4K",
      ["Dolby Vision", "Atmos", "5.1"],
      8.4,
      ready,
      "Real-Debrid",
      "2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX",
      ["EN audio", "EN subs", "TR subs"],
      2,
      ["Plays directly on this TV, no conversion", "Ready now, cached on Real-Debrid", "Your languages: English audio, Turkish subtitles"],
    ),
    rows: [
      row("bestQuality", "4K", ["HDR10", "Atmos", "5.1"], 7.9, ready, "TorBox", "2160p.WEB-DL.DDP5.1.Atmos.HDR10.H.265-NTb", ["EN audio", "EN subs"], 1),
      row("bestQuality", "4K", ["Dolby Vision", "Atmos", "5.1"], 8.1, ready, "Real-Debrid", "2160p.WEB-DL.DDP5.1.Atmos.DV.H.265-playWEB", ["EN audio", "ES subs"]),
      row("bestQuality", "4K", ["HDR10", "5.1"], 6.2, ready, "Real-Debrid", "2160p.WEB.H265-EDITH", ["EN audio", "EN subs"]),
      row("bestQuality", "4K", ["HDR10"], 5.4, ready, "TorBox", "2160p.WEB.HDR.H265-GGEZ", ["EN audio"]),
      row("smallerFaster", "1080p", ["Atmos", "5.1"], 3.9, ready, "Real-Debrid", "1080p.WEB-DL.DDP5.1.Atmos.H.264-FLUX", ["EN audio", "TR subs"], 1),
      row("smallerFaster", "1080p", ["5.1"], 2.4, ready, "TorBox", "1080p.WEB.H264-SuccessfulCrab", ["EN audio", "EN subs"]),
      row("smallerFaster", "1080p", ["HEVC", "Stereo"], 0.612, ready, "Real-Debrid", "1080p.HEVC.x265-MeGusta", ["EN audio"]),
      row("otherLanguages", "1080p", ["5.1"], 3.1, ready, "Real-Debrid", "MULTi.1080p.WEB.H264-FW", ["FR audio", "EN audio", "FR subs"]),
      row("otherLanguages", "1080p", ["5.1"], 3.6, ready, "TorBox", "ITA.ENG.1080p.WEB-DL.DDP5.1.H.264-MeM", ["IT audio", "EN audio", "IT subs"]),
      row("needsDownload", "4K", ["HDR10", "5.1"], 6.2, { state: "torrent", seeds: 214 }, "Torrentio", "2160p.WEB.H265-EDITH", ["EN audio"]),
      row("needsDownload", "1080p", ["HEVC", "5.1"], 1.1, { state: "needsDownload" }, "Real-Debrid", "1080p.10bit.WEBRip.6CH.x265-PSA", ["EN audio"]),
    ],
    hidden: { count: 4, summary: "2 suspected fakes, 1 dead torrent, 1 paid link" },
  };
}

/** Status in words; the dot color is never the only signal. */
export function statusWords(s: VersionStatus, via: string): { tone: "ok" | "warn"; text: string } {
  switch (s.state) {
    case "ready":
      return { tone: "ok", text: `Ready on ${via}` };
    case "torrent":
      return { tone: "warn", text: `Torrent, ${s.seeds} seeds` };
    case "needsDownload":
      return { tone: "warn", text: "Needs download" };
  }
}

/** Groups rows in display order, dropping empty groups. */
export function grouped(rows: readonly VersionRow[]): { group: VersionGroup; rows: VersionRow[] }[] {
  return groupOrder
    .map((group) => ({ group, rows: rows.filter((r) => r.group === group) }))
    .filter((g) => g.rows.length > 0);
}
