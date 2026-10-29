/** Nuvio plugin repositories' words (plans/plugins-and-marketplace.md 7.7, 7.8), from
 * apps/shared/plugin-words.json, which every app reads. */

import words from "../../../shared/plugin-words.json";
import type { PluginRepoInfo } from "../core/generated/PluginRepoInfo";
import type { VersionRow } from "../core/generated/VersionRow";

export const pluginWords = words;

/** `template` with each `{name}` replaced by its value as plain text. */
export const fill = (template: string, values: Record<string, string | number>) =>
  template.replace(/\{(\w+)\}/g, (all, key: string) => (key in values ? String(values[key]) : all));

/** "Dizibox, plays from cdn.example.com" (4.4; the quality is the row's badge): a plugin row says where it plays from. */
export const pluginRowLine = (row: VersionRow) =>
  fill(words.row, { quality: row.quality, source: row.sourceName ?? "", host: row.host ?? "" });

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
