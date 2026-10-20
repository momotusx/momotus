/** Spatial navigation for remotes and keyboards.
 *
 * Pure part: pick the best candidate in a direction from rectangles.
 * Rules (tvOS and Android TV behavior): only candidates beyond the current
 * edge in that direction; prefer overlap on the cross axis; distance on the
 * main axis dominates; a group (row) remembers its last focused member and
 * entering the group restores it. Left and right stay inside a row; an
 * element that is not in one (a lone button, a vertical list) takes the
 * nearest element that way when nothing lies beside it. */

export type Direction = "up" | "down" | "left" | "right";

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Candidate<K> {
  key: K;
  rect: Rect;
  group?: string;
  /** First member of the stack this candidate is in (`data-fentry`): moving
   * into the stack from outside lands there, not on the nearest member. */
  entry?: K;
}

const right = (r: Rect) => r.left + r.width;
const bottom = (r: Rect) => r.top + r.height;

function overlap(a0: number, a1: number, b0: number, b1: number): number {
  return Math.max(0, Math.min(a1, b1) - Math.max(a0, b0));
}

/** Lower is better; `null` when the candidate is not in that direction.
 * `diagonal` lets left and right reach what is not beside `from`. */
export function score(from: Rect, to: Rect, dir: Direction, diagonal = false): number | null {
  const horizontal = dir === "left" || dir === "right";
  let main: number;
  switch (dir) {
    case "right":
      main = to.left - right(from);
      break;
    case "left":
      main = from.left - right(to);
      break;
    case "down":
      main = to.top - bottom(from);
      break;
    case "up":
      main = from.top - bottom(to);
      break;
  }
  // Allow a small overlap so tightly packed tiles still count as adjacent.
  if (main < -Math.min(from.width, from.height) / 2) return null;
  const crossOverlap = horizontal
    ? overlap(from.top, bottom(from), to.top, bottom(to))
    : overlap(from.left, right(from), to.left, right(to));
  const fromCenter = horizontal ? from.top + from.height / 2 : from.left + from.width / 2;
  const toCenter = horizontal ? to.top + to.height / 2 : to.left + to.width / 2;
  const crossDistance = Math.abs(toCenter - fromCenter);
  // Left and right stay inside the row: no diagonal jumps at a row end.
  if (horizontal && crossOverlap === 0 && !diagonal) return null;
  const aligned = crossOverlap > 0 ? 0 : 1;
  return aligned * 1e6 + Math.max(0, main) * 4 + crossDistance;
}

/** Picks the next focus target. `memory` maps a group to its last key. */
export function pick<K>(
  current: Candidate<K>,
  candidates: readonly Candidate<K>[],
  dir: Direction,
  memory: ReadonlyMap<string, K>,
): K | null {
  const nearest = (diagonal: boolean) => {
    let best: Candidate<K> | null = null;
    let bestScore = Infinity;
    for (const c of candidates) {
      if (c.key === current.key) continue;
      const s = score(current.rect, c.rect, dir, diagonal);
      if (s !== null && s < bestScore) {
        best = c;
        bestScore = s;
      }
    }
    return best;
  };
  // In a row (its group has another member beside it), left and right stop
  // at the row's end. A lone element, or one in a vertical list, takes the
  // nearest element that way when nothing lies beside it (Versions: Play on
  // the left, a short list up on the right).
  // Beside: sharing most of the shorter one's height (a focused list row that
  // grows over its neighbour by a few pixels is still a list, not a row).
  const beside = (c: Candidate<K>) =>
    overlap(current.rect.top, bottom(current.rect), c.rect.top, bottom(c.rect)) > Math.min(current.rect.height, c.rect.height) / 2;
  const inRow = current.group !== undefined && candidates.some((c) => c.key !== current.key && c.group === current.group && beside(c));
  const horizontal = dir === "left" || dir === "right";
  const best = nearest(false) ?? (horizontal && !inRow ? nearest(true) : null);
  if (!best) return null;
  if (best.entry !== undefined && best.entry !== current.entry) return best.entry;
  const leavingGroup = best.group !== undefined && best.group !== current.group;
  if (leavingGroup) {
    const remembered = memory.get(best.group as string);
    if (remembered !== undefined && candidates.some((c) => c.key === remembered)) return remembered;
  }
  return best.key;
}
