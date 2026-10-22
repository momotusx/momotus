import type { HomeRow } from "../core/generated/HomeRow";

/** Every lane has ended with no tile and at least one failed (most often no network): Home offers one
 * Try again instead of a column of failures. The same rule as Android's `allFailed`. */
export const allFailed = (rows: HomeRow[]): boolean =>
  rows.length > 0 &&
  rows.every((r) => r.tiles.length === 0 && r.state.state !== "loading" && r.state.state !== "slow") &&
  rows.some((r) => r.state.state === "failed");

/** The titles two or more rows (Home's rows, Search's sections, Home rows in Settings, Discover's
 * catalogs) on a screen share. A row shows its title alone; only these also name their addon, to
 * tell them apart (the owner, 2026-10-22). */
export function sharedTitles(rows: readonly { title: string }[]): Set<string> {
  const seen = new Set<string>();
  const twice = new Set<string>();
  for (const r of rows) (seen.has(r.title) ? twice : seen).add(r.title);
  return twice;
}
