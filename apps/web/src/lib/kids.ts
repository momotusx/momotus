/** What a kids session sees (plans/m11-kids.md section 5): its own list, never the screens that change sources,
 * the account or profiles, nor Discover and Calendar's whole catalog. The core refuses all of them with `kids`;
 * these keep them off screen. */

import type { ProfilesSnapshot } from "../core/generated/ProfilesSnapshot";
import { current } from "./profiles";
import type { SectionId } from "./settings-sections";

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
};
