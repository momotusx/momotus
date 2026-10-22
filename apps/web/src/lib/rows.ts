import type { HomeRow } from "../core/generated/HomeRow";

/** Every lane has ended with no tile and at least one failed (most often no network): Home offers one
 * Try again instead of a column of failures. The same rule as Android's `allFailed`. */
export const allFailed = (rows: HomeRow[]): boolean =>
  rows.length > 0 &&
  rows.every((r) => r.tiles.length === 0 && r.state.state !== "loading" && r.state.state !== "slow") &&
  rows.some((r) => r.state.state === "failed");

/** A row's states in words that name no addon, not even to tell two rows apart (the owner,
 * 2026-10-22; design/DECISION.md, Shelves): addon names show only on the Addons pages and as
 * Customize Home's quiet second line. */
export const rowWords = {
  failed: "This row did not load.",
  retrying: "Trying this row again.",
  slow: "Slow to answer, still trying",
  empty: "Nothing in this row right now.",
  searchFailed: "These results did not load.",
} as const;
