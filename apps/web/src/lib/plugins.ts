/** Nuvio plugin repositories' words (plans/plugins-and-marketplace.md 7.7, 7.8), from
 * apps/shared/plugin-words.json, which every app reads. */

import words from "../../../shared/plugin-words.json";
import type { ListedRepo } from "../core/generated/ListedRepo";
import type { PluginRepoInfo } from "../core/generated/PluginRepoInfo";

export const pluginWords = words;

/** `template` with each `{name}` replaced by its value as plain text. */
export const fill = (template: string, values: Record<string, string | number>) =>
  template.replace(/\{(\w+)\}/g, (all, key: string) => (key in values ? String(values[key]) : all));

/** "Adds 8 providers: A, B, C, D, E and 3 more." for the consent sheet. */
export function addsProviders(names: string[]): string {
  const n = names.length;
  const shown = names.slice(0, 5);
  const rest = n - shown.length;
  const list = rest > 0 ? `${shown.join(", ")} and ${rest} more` : shown.length < 2 ? shown.join("") : `${shown.slice(0, -1).join(", ")} and ${shown.at(-1)}`;
  return fill(words.sheetAdds, { what: `${n} ${n === 1 ? "provider" : "providers"}: ${list}` });
}

/** The Settings row's line: how many repositories and providers are on. */
export function pluginsSummary(repos: PluginRepoInfo[]): string {
  if (!repos.length) return words.summaryNone;
  const on = repos.filter((r) => r.enabled).reduce((n, r) => n + r.providers.filter((p) => p.enabled).length, 0);
  const n = repos.length;
  return `${n} ${n === 1 ? "repository" : "repositories"}, ${on} ${on === 1 ? "provider" : "providers"} on`;
}

const kindWords: Record<string, string> = { movie: "films", series: "series" };

/** "8 providers for films and series. Languages: tr, en." for a listed repository's card. */
export function directoryLine(r: ListedRepo): string {
  const n = r.providers.length;
  const kinds = r.types.map((t) => kindWords[t] ?? t);
  const head = `${n} ${n === 1 ? "provider" : "providers"}${kinds.length ? ` ${fill(words.forKinds, { kinds: kinds.join(" and ") })}` : ""}.`;
  return r.languages.length ? `${head} ${fill(words.languages, { languages: r.languages.join(", ") })}` : head;
}

/** Lower case, accents dropped: "Türkçe" finds "turkce". */
const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** Whether every word of `query` is in the repository's name, its providers' names or its languages. */
export function matchesRepo(r: ListedRepo, query: string): boolean {
  const text = fold([r.name, ...r.providers, ...r.languages].join(" "));
  return fold(query)
    .split(/\s+/)
    .filter(Boolean)
    .every((q) => text.includes(q));
}
