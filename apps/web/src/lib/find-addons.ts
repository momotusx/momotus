/** What a Find addons card shows and does, from the core's entry. The core
 * sends facts (`installed`, `configuration`); these rules turn them into one
 * action, the same on every app (plans/m12-addon-discovery.md 3.2). */

import type { ListedAddon } from "../core/generated/ListedAddon";
import { findAddons as w } from "./words";

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
 * Configure; setup optional, Install and Configure; else Install. A TV has no
 * Configure: the addon's website is set up on a phone or computer. */
export type Action =
  | { kind: "none"; word: string }
  | { kind: "turnOn"; url: string; word: string }
  | { kind: "configure" }
  | { kind: "install"; configure: boolean };

export function action(entry: ListedAddon, tv: boolean): Action {
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
      return { kind: "install", configure: !tv && entry.configuration === "optional" && entry.configureUrl !== null };
  }
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
