/** Plain-language copy for the core's typed facts. The core sends enums and
 * codes; every UI turns them into the same words. */

import type { Group } from "../core/generated/Group";
import type { HiddenSummary } from "../core/generated/HiddenSummary";
import type { HideReason } from "../core/generated/HideReason";
import type { Reason } from "../core/generated/Reason";
import type { Tile } from "../core/generated/Tile";
import type { VersionStatus } from "../core/generated/VersionStatus";

export function reasonText(r: Reason, status: VersionStatus): string {
  switch (r) {
    case "readyNow":
      return status.kind === "ready" && status.service ? `Ready now, cached on ${status.service}` : "Ready now";
    case "playsDirectly":
      return "Plays directly on this device";
    case "matchesScreen":
      return "Matches your screen";
    case "sameReleaseAsLast":
      return "Same release as the last episode";
    case "yourLanguage":
      return "In your language";
    case "smallerFaster":
      return "Smaller and faster to start";
  }
}

export const groupLabel: Record<Group, string> = {
  best: "Best quality",
  smallerFaster: "Smaller and faster",
  otherLanguages: "Other languages",
  needsDownload: "Needs download",
};

export const groupOrder: Group[] = ["best", "smallerFaster", "otherLanguages", "needsDownload"];

const hideText: Record<HideReason, [one: string, many: string]> = {
  wrongTitle: ["is for another title", "are for other titles"],
  wrongEpisode: ["is another episode", "are other episodes"],
  excluded: ["matches your avoid list", "match your avoid list"],
  tooLarge: ["is over your size limit", "are over your size limit"],
  unplayable: ["cannot play on this device", "cannot play on this device"],
  unsupportedSource: ["uses a source this app cannot open yet", "use a source this app cannot open yet"],
};

/** "19 are for other titles, 1 is another episode". */
export function hiddenSummary(h: HiddenSummary): string {
  return h.byReason.map(([reason, n]) => `${n} ${hideText[reason][n === 1 ? 0 : 1]}`).join(", ");
}

export type Tone = "ok" | "warn" | "bad" | "idle";

/** Status in words with a tone for its dot; the word always carries it. */
export function statusText(s: VersionStatus): { tone: Tone; text: string } {
  switch (s.kind) {
    case "ready":
      return { tone: "ok", text: s.service ? `Ready on ${s.service}` : "Ready" };
    case "direct":
      return { tone: "ok", text: "Direct link" };
    case "torrent":
      return { tone: "warn", text: s.seeds === null ? "Torrent" : `Torrent, ${s.seeds} seeds` };
    case "needsDownload":
      return { tone: "warn", text: s.service ? `Needs download on ${s.service}` : "Needs download" };
  }
}

const names = (() => {
  try {
    return new Intl.DisplayNames(["en"], { type: "language" });
  } catch {
    return null;
  }
})();

/** "fr" or "fre" to "French"; unknown codes stay as given, upper-cased. */
export function languageName(code: string): string {
  const c = code.toLowerCase();
  const iso1: Record<string, string> = { eng: "en", tur: "tr", fre: "fr", fra: "fr", ger: "de", deu: "de", spa: "es", ita: "it", por: "pt", rus: "ru", jpn: "ja", kor: "ko", chi: "zh", zho: "zh", ara: "ar", dut: "nl", nld: "nl", pol: "pl", swe: "sv", hin: "hi", gre: "el", ell: "el", rum: "ro", ron: "ro", hun: "hu", cze: "cs", ces: "cs", heb: "he", per: "fa", fas: "fa", pob: "pt-BR" };
  try {
    const name = names?.of(iso1[c] ?? c);
    if (name && name.toLowerCase() !== c) return name;
  } catch {
    // Not a language code: fall through.
  }
  return code.toUpperCase();
}

/** Continue Watching tiles (in progress, or Next Up) carry their one caption
 * from the core ("S2:E3 · 36 min left", "Next: S1:E2"); it is shown as is.
 * Catalog tiles show none. */
export const caption = (tile: Tile): string | null =>
  tile.progress !== null || tile.releaseInfo?.startsWith("Next:") ? tile.releaseInfo : null;

/** A version's headline as the core words it: the resolution, else the
 * source ("WEB-DL"), else "Quality not stated"; never "unknown". */
export const qualityText = (q: string) => (q === "Unknown" ? "Quality not stated" : q);
/** The same fact in a table cell or a small value: one word that never wraps. */
export const qualityCell = (q: string) => (q === "Unknown" || q === "Quality not stated" ? "Unknown" : q);

export function kindLabel(kind: string): string {
  const labels: Record<string, string> = { movie: "Movie", series: "Series", channel: "Channel", tv: "Live TV", anime: "Anime" };
  return labels[kind] ?? kind.charAt(0).toUpperCase() + kind.slice(1);
}

/** What an addon provides, in words. */
export function providesText(p: string): string {
  const labels: Record<string, string> = {
    catalog: "Catalogs",
    meta: "Details",
    stream: "Streams",
    subtitles: "Subtitles",
    addon_catalog: "Addon lists",
  };
  return labels[p] ?? p;
}

/**
 * Why pairing stopped, in words. The core's own pairing reasons are fixed
 * sentences and are kept; a connection or address error is replaced, since its
 * text can name the server's address (Android's `SyncCopy.pairingProblem`).
 */
export function pairingProblem(reason: string): string {
  if (reason.startsWith("pairing: ")) {
    const why = reason.slice("pairing: ".length);
    return `${why.charAt(0).toUpperCase()}${why.slice(1)}.`;
  }
  if (reason.startsWith("the relay must use https")) return "The sync server address must start with https.";
  if (reason.startsWith("invalid url")) return "That is not a sync server address. It looks like https://sync.example.org.";
  if (reason.startsWith("blocked private address")) return "This device does not connect to that address.";
  return "The sync server did not answer. Check the address and try again.";
}

/** The server's host name only: never a path, a query or a user name. */
export function serverHost(url: string): string {
  try {
    return new URL(url).hostname || "the sync server";
  } catch {
    return "the sync server";
  }
}

/** An address with a host and no user name or password in it. */
export function plainAddress(url: string): boolean {
  try {
    const u = new URL(url);
    return u.hostname !== "" && u.username === "" && u.password === "";
  } catch {
    return false;
  }
}
