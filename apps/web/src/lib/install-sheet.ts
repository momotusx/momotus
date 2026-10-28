/** The addon install sheet's words and its Adds line (plans/addon-install-sheet.md), from
 * apps/shared/install-sheet.json, which every app reads and tests. */

import sheet from "../../../shared/install-sheet.json";

export const sheetWords = sheet.words;

const resources: Record<string, string> = sheet.resources;
const kinds: Record<string, string> = sheet.kinds;
// Manifest words are the addon's own: "constructor" or "__proto__" is a word, not a key of the object.
// The build targets ES2020 (older TV browsers), so no Object.hasOwn.
const own = (words: Record<string, string>, key: string) => (Object.prototype.hasOwnProperty.call(words, key) ? words[key] : undefined);

/** "a", "a and b", "a, b and c". */
const join = (xs: string[]) => (xs.length < 2 ? (xs[0] ?? "") : `${xs.slice(0, -1).join(", ")} and ${xs[xs.length - 1]}`);

/** `template` with each `{name}` replaced by its value as plain text (a "$&" in a name stays "$&"). */
const fill = (template: string, values: Record<string, string>) => template.replace(/\{(\w+)\}/g, (all, key: string) => values[key] ?? all);

/** "Adds sources and subtitles for movies and series.", or "" when the manifest names nothing the app shows. */
export function addsLine(provides: string[], types: string[]): string {
  const what = [...new Set(provides.map((p) => own(resources, p)).filter((w): w is string => !!w))];
  if (!what.length) return "";
  const named = [...new Set(types.map((t) => t.trim().toLowerCase()).filter(Boolean).map((t) => own(kinds, t) ?? t))];
  const onlyLists = what.length === 1 && what[0] === resources.addon_catalog;
  if (!named.length || onlyLists) return fill(sheetWords.adds, { what: join(what) });
  return fill(sheetWords.addsFor, { what: join(what), kinds: join(named) });
}

export const sheetTitle = (name: string) => fill(sheetWords.title, { name });
export const sheetFrom = (host: string) => fill(sheetWords.from, { host });
