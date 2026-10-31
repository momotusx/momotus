/** The language picker's rules that are not the core's (plans/onboarding-language-picker.md): what a
 * tap does in Settings, the words for it, the row's summary, and the query the TV keyboard edits. The
 * search, its order and the suggestions are the core's (`languagePicker`). */

import { languageTag } from "./words";

/** What a tap on a language does in Settings. */
export type TapAction = "add" | "remove" | "default";

/** Settings' tap rule. Subtitles: a language that is off goes in (the visit's first new pick goes first,
 * later ones of the same visit after it, in tap order); a language added this visit comes out again (undo);
 * an older pick that is not first moves first (the default is the most recently chosen); the first, older
 * one comes out. Audio keeps its own order: a new pick goes last, a tap on a chosen one removes it. `added`
 * holds the visit's additions. */
export function tapAction(list: readonly string[], code: string, added: ReadonlySet<string>, audio: boolean): TapAction {
  const at = list.indexOf(code);
  if (at < 0) return "add";
  if (audio || added.has(code) || at === 0) return "remove";
  return "default";
}

export function tap(list: readonly string[], code: string, added: ReadonlySet<string>, audio: boolean): { list: string[]; added: Set<string> } {
  const next = new Set(added);
  const action = tapAction(list, code, added, audio);
  if (action === "remove") {
    next.delete(code);
    return { list: list.filter((c) => c !== code), added: next };
  }
  if (action === "default") return { list: [code, ...list.filter((c) => c !== code)], added: next };
  next.add(code);
  if (audio) return { list: [...list, code], added: next };
  // Right after the visit's last addition (tap order), else first.
  const at = Math.max(-1, ...list.map((c, i) => (added.has(c) ? i : -1))) + 1;
  return { list: [...list.slice(0, at), code, ...list.slice(at)], added: next };
}

export const tapWords: Record<TapAction, string> = { add: "Add", remove: "Remove", default: "Make default" };

/** The Settings picker's one hint line. */
export const settingsHint = (audio: boolean) =>
  audio
    ? "Tap a language to add it; the first one is used first. Tap a chosen one to remove it."
    : "Tap a language to add it. Tap a chosen one to make it the default; tap the default to remove it.";

/** A Settings row's value: the first language and how many follow it, or `none`. */
export function summary(names: readonly string[], none: string): string {
  const [first, ...rest] = names;
  if (!first) return none;
  return rest.length ? `${first} first, ${rest.length} more` : `${first} first`;
}

/** The TV keyboard's keys, in order. */
export const KEYS = ["space", ..."abcdefghijklmnopqrstuvwxyz", "delete"] as const;
export type Key = (typeof KEYS)[number];

/** The query after a key: a letter types itself, space a space, delete the last character. */
export function press(query: string, key: Key): string {
  if (key === "delete") return query.slice(0, -1);
  if (key === "space") return query.endsWith(" ") || !query ? query : `${query} `;
  return query + key;
}

/** "12 found", "1 found". */
export const found = (n: number) => `${n} found`;

/** Settings' Second line (plans/dual-subtitles.md 5.5): a language in the first subtitle language's primary language
 * (`first`, as the profile stores it, "eng" or "pt-BR") is the first line's and cannot be picked there. */
export const onFirstLine = (code: string, first: string | undefined): boolean => !!first && primary(code) === primary(first);
const primary = (code: string) => languageTag(code).split("-")[0];
