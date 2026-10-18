import type { HomeRow } from "../core/generated/HomeRow";

/** Every lane has ended with no tile and at least one failed (most often no network): Home offers one
 * Try again instead of a column of failures. The same rule as Android's `allFailed`. */
export const allFailed = (rows: HomeRow[]): boolean =>
  rows.length > 0 &&
  rows.every((r) => r.tiles.length === 0 && r.state.state !== "loading" && r.state.state !== "slow") &&
  rows.some((r) => r.state.state === "failed");
