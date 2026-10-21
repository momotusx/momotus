/** The player's loading layer (plans/player-polish.md 1.2): how far the
 * logo is filled, and the words under it. */

/** Seconds of video ahead of the playhead the player wants before it plays:
 * a link's start and a stall fill against this. */
export const START_AHEAD_S = 3;

/** The share of `need` seconds buffered ahead of `at`, 0 to 1. */
export function bufferShare(buffered: [number, number][], at: number, need = START_AHEAD_S): number {
  const range = buffered.find(([a, b]) => at >= a - 0.25 && at <= b);
  return range ? Math.min(1, Math.max(0, (range[1] - at) / need)) : 0;
}

/** The fill: the core's progress (`held`, the last it sent) until the player
 * has the URL, then the player's own buffer (`share`) fills what is left.
 * Never 1 before the first frame. */
export function startFill(held: number, share: number): number {
  const core = Math.min(0.99, Math.max(0, held));
  return Math.min(0.99, core + (1 - core) * Math.min(1, Math.max(0, share)));
}

/** The lines under the logo: the player's own status ("Starting 1080p,
 * 1.5 GB from Torrentio"), then the core's caption once it has one, each
 * once. */
export function loadingLines(status: string | null, caption: string | null | undefined): string[] {
  const lines: string[] = [];
  for (const l of [status, caption]) if (l && !lines.includes(l)) lines.push(l);
  return lines;
}
