/** What a kids session sees (plans/m11-kids.md section 5): its own list, never the screens that change sources,
 * the account or profiles, nor Discover and Calendar's whole catalog. The core refuses all of them with `kids`;
 * these keep them off screen. */

import type { KidsKeysSnapshot } from "../core/generated/KidsKeysSnapshot";
import type { ProfileInfo } from "../core/generated/ProfileInfo";
import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import type { TitleState } from "../core/generated/TitleState";
import { current } from "./profiles";
import type { SectionId } from "./settings-sections";
import { kidsLine } from "./words";

/** The session runs a kids profile: the core's session flag, or the active profile's own. */
export const kidsSession = (s: ProfilesSnapshot | null | undefined) => !!s && (s.kids || current(s)?.kids === true);

/** Settings sections a kids profile does not have: Sources, Account, Home rows and Downloads. */
export const KIDS_HIDDEN_SECTIONS: readonly SectionId[] = ["sources", "account", "home", "downloads"];

const PAGES = ["/discover", "/calendar", "/welcome", "/addons", "/recommended-addons", "/find-addons", "/plugins", "/import", "/pair", "/sync", "/home-rows", "/debrid", "/trakt"];
const UNDER = ["/sign-in", "/sync/", "/profiles/"];

/** A page a kids session never opens (a typed address goes Home): Discover, Calendar, every source, account and
 * profile-editing page, and the hidden Settings sections. The picker (`/profiles`) stays: it is the way out. */
export function kidsBlocked(path: string): boolean {
  if (PAGES.includes(path) || UNDER.some((p) => path.startsWith(p))) return true;
  const section = path.startsWith("/settings/") ? path.slice("/settings/".length) : null;
  return !!section && KIDS_HIDDEN_SECTIONS.includes(section as SectionId);
}

/** Words a kids session reads in place of the addon ones (plans/m11-kids.md 6.4). */
export const kidsWords = {
  searchField: "Search your shows",
  searchNone: "That isn't on your list. Ask a grown-up.",
  empty: "Nothing here yet. Ask a grown-up to pick some shows for you.",
  titleRefused: "That isn't on your list",
  ask: "Ask a grown-up.",
};

/** A title the core refused for a kids profile (`notAllowed`): said at once with Back only, never Loading and never
 * a way to sources (QA round 1, Q7 row 28). */
export const titleRefused = (state: TitleState | undefined) => state?.state === "notAllowed";

/** The owner's unlocked session, out of Incognito: the one that manages kids profiles (the core answers `notOwner` or
 * `kids` to every other). */
export const managesKids = (s: ProfilesSnapshot | null | undefined) => !!s && !s.locked && !kidsSession(s) && current(s)?.isOwner === true;

/** The main profile, whose PIN lets a kids session out (plans/m11-kids.md 7). */
export const ownerOf = (s: ProfilesSnapshot | null | undefined) => s?.profiles.find((p) => p.isOwner);

/** Whose PIN the picker asks when `p` is picked: out of a kids session, for any other profile, the main profile's
 * ("main"; the core checks that one, never the target's); otherwise the profile's own ("own"); null when none is asked.
 * A main profile with no PIN asks nothing here, and the core then refuses the exit (`kidsNeedPin`). */
export function pinAsked(s: ProfilesSnapshot | null | undefined, p: ProfileInfo): "main" | "own" | null {
  if (kidsSession(s) && p.id !== s?.active) return ownerOf(s)?.hasPin ? "main" : null;
  return p.hasPin ? "own" : null;
}

/** One kids profile and whether a title is on its list, from the core's `kidsKeys` (never a set the app keeps). */
export type KidsChoice = { profile: string; name: string; allowed: boolean };
export const kidsChoices = (keys: KidsKeysSnapshot | null | undefined, kind: string, id: string): KidsChoice[] =>
  (keys?.profiles ?? []).map((k) => ({ profile: k.profile, name: k.name, allowed: k.keys.includes(`${kind}:${id}`) }));

/** What an Allow the core refused says, by its code; null for any other failure (its own words). */
export function allowProblem(code: string | undefined, name: string): string | null {
  if (code === "mismatch") return kidsLine("mismatch");
  if (code === "noDetails") return kidsLine("noDetails");
  if (code === "listFull") return kidsLine("listFull", { name });
  return null;
}

/** The screen on `path` shows one of `items`, titles the core just took off the kids list: it leaves. */
export function takenOffHere(path: string, items: readonly string[]): boolean {
  const [first, , id] = path.split("/").filter(Boolean);
  return (first === "title" || first === "play") && !!id && items.includes(decodeURIComponent(id));
}

/** A policy write (create a kids profile, allow, remove, make standard), then a sync round at once when an account is
 * joined, so the change leaves this device in seconds (plans/m11-kids.md 6.1 and 8). */
export async function writePolicy<T>(write: () => Promise<T>, sync: { joined: () => boolean; now: () => unknown }): Promise<T> {
  const done = await write();
  if (sync.joined()) void sync.now();
  return done;
}
