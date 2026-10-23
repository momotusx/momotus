import { expect, test } from "bun:test";
import type { DiscoverSnapshot } from "../core/generated/DiscoverSnapshot";
import { discoverLinkStep } from "./discover-link";

const snap = (over: Partial<DiscoverSnapshot>): DiscoverSnapshot =>
  ({ kinds: [["movie", "Movies"], ["series", "Series"]], kind: "movie", catalogs: [], catalog: null, genres: [], genre: null, genreRequired: false, items: [], state: { state: "ready" }, ...over }) as DiscoverSnapshot;

test("a genre link selects the kind first, then the genre once the kind's catalog lists it", () => {
  const link = { kind: "series", genre: "drama" };
  expect(discoverLinkStep(link, snap({ kind: "movie", genres: ["Drama"] }))).toEqual({ kind: "series" });
  // The snapshot from before the switch, or one still loading its genres: wait.
  expect(discoverLinkStep(link, snap({ kind: "series", genres: [], state: { state: "loading" } as DiscoverSnapshot["state"] }))).toBe("wait");
  // Matched without regard to case, then done once selected.
  expect(discoverLinkStep(link, snap({ kind: "series", genres: ["Comedy", "Drama"] }))).toEqual({ genre: "Drama" });
  expect(discoverLinkStep(link, snap({ kind: "series", genres: ["Comedy", "Drama"], genre: "Drama" }))).toBe("done");
});

test("a genre the catalog does not list, or a kind Discover lacks, leaves the page as it is", () => {
  expect(discoverLinkStep({ kind: "series", genre: "Western" }, snap({ kind: "series", genres: ["Drama"] }))).toBe("done");
  expect(discoverLinkStep({ kind: "anime", genre: "Drama" }, snap({ kind: "movie" }))).toBe("done");
  expect(discoverLinkStep({ kind: "series" }, snap({ kind: "series" }))).toBe("done");
});
