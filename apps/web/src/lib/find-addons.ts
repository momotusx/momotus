/** What a Find addons card shows and does, from the core's entry. The core
 * sends facts (`installed`, `configuration`); these rules turn them into one
 * action, the same on every app (plans/m12-addon-discovery.md 3.2). */

import type { ListedAddon } from "../core/generated/ListedAddon";
import { kindLabel, providesText, findAddons as w } from "./words";

export type Filter = "all" | "sources" | "subtitles" | "catalogs";

const provides: Record<Exclude<Filter, "all">, string> = { sources: "stream", subtitles: "subtitles", catalogs: "catalog" };

export const filters: { value: Filter; label: string }[] = [
  { value: "all", label: w.all },
  { value: "sources", label: w.sources },
  { value: "subtitles", label: w.subtitles },
  { value: "catalogs", label: w.catalogs },
];

export const shows = (entry: ListedAddon, filter: Filter) => filter === "all" || entry.provides.includes(provides[filter]);

/** "No subtitle addons in this list." */
export function filterEmpty(filter: Exclude<Filter, "all">): string {
  const kind = { sources: w.kindSources, subtitles: w.kindSubtitles, catalogs: w.kindCatalogs }[filter];
  return w.filterEmpty.replace("{kind}", kind);
}

/** The card's action, first match: installed and on, none; off, Turn on that
 * link (a configured copy's when only a copy is installed); setup required,
 * Configure; setup optional, Install and Configure; else Install. A TV shows
 * no Configure (its screen draws the help line instead). */
export type Action =
  | { kind: "none"; word: string }
  | { kind: "turnOn"; url: string; word: string }
  | { kind: "configure" }
  | { kind: "install"; configure: boolean };

export function action(entry: ListedAddon): Action {
  switch (entry.installed) {
    case "on":
      return { kind: "none", word: w.installed };
    case "copyOn":
      return { kind: "none", word: w.setUp };
    case "off":
      return { kind: "turnOn", url: entry.key, word: w.off };
    case "copyOff":
      return entry.copyKey ? { kind: "turnOn", url: entry.copyKey, word: w.setUpOff } : { kind: "none", word: w.setUpOff };
    case "absent":
      if (entry.configuration === "required") return { kind: "configure" };
      return { kind: "install", configure: entry.configuration === "optional" };
  }
}

/** The action with the card's last result: an install refused as needing
 * setup asks for Configure, until the entry is installed some other way (a
 * pasted configured link), which wins. */
export function cardAction(entry: ListedAddon, lastCode: string | undefined): Action {
  if (entry.installed === "absent" && lastCode === "needsSetup") return { kind: "configure" };
  return action(entry);
}

const kindsWords: Record<string, string> = { movie: "movies", series: "series", channel: "channels", tv: "live TV", anime: "anime" };

/** "Subtitles for movies and series". */
export function providesLine(entry: ListedAddon): string {
  const what = entry.provides.map(providesText).join(", ");
  const kinds = entry.types.map((t) => kindsWords[t] ?? kindLabel(t).toLowerCase());
  if (!kinds.length) return what;
  const last = kinds.length > 1 ? ` and ${kinds.pop()}` : "";
  return `${what} for ${kinds.join(", ")}${last}`;
}

/** Names shown by more than one card of a list: their cards also say the host. */
export function repeatedNames(entries: ListedAddon[]): Set<string> {
  const seen = new Set<string>();
  const twice = new Set<string>();
  for (const e of entries) (seen.has(e.name) ? twice : seen).add(e.name);
  return twice;
}

/** The mark on the brand surface: the name's first letter or digit, symbols
 * skipped; no remote logo, so opening a list contacts no image host. */
export const initial = (name: string) => [...name].find((c) => /[\p{L}\p{N}]/u.test(c))?.toLocaleUpperCase("en") ?? "";

/** What the card does, in words, for a screen reader: "Install OpenSubtitles
 * v3, subtitles", with the host when another card in the list has the same
 * name. An installed card says its state instead of an action. */
export function spoken(entry: ListedAddon, act: Action, sameName: boolean, providesWords: string): string {
  const verb = act.kind === "none" ? null : act.kind === "turnOn" ? w.turnOn : act.kind === "configure" ? w.configure : w.install;
  const name = verb ? `${verb} ${entry.name}` : entry.name;
  const parts = [name, sameName ? w.fromHost.replace("{host}", entry.host) : null, providesWords.toLowerCase(), act.kind === "none" ? act.word.toLowerCase() : null];
  return parts.filter(Boolean).join(", ");
}
