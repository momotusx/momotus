import type { DiscoverSnapshot } from "../core/generated/DiscoverSnapshot";

/** A link into Discover (a title's genre chip): `/discover?kind=series&genre=Drama`. */
export type DiscoverLink = { kind?: string; genre?: string };

/** The next step to apply a link, as Apple's rule (plans/design-v3-phase34-web.md, the info column):
 * the kind first; then the genre once that kind's catalog lists it, matched without regard to case;
 * a genre the catalog does not list leaves the kind alone ("done"). "wait" while the snapshot is still
 * the one before the kind switch, or the genre list has not come. */
export function discoverLinkStep(link: DiscoverLink, s: DiscoverSnapshot): { kind: string } | { genre: string } | "wait" | "done" {
  if (link.kind && s.kind !== link.kind) return s.kinds.some(([k]) => k === link.kind) ? { kind: link.kind } : "done";
  if (!link.genre) return "done";
  if (s.genres.length === 0) return s.state.state === "loading" ? "wait" : "done";
  const match = s.genres.find((g) => g.toLowerCase() === link.genre?.toLowerCase());
  if (!match || s.genre === match) return "done";
  return { genre: match };
}
