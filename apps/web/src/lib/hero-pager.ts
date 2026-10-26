/** Home's hero carousel as plain state (plans/appletv-polish.md 4.1): one progress value drives both
 * the active dot's fill and the advance, so a pause or reduced motion can never make them drift. */

export type PagerWhy = "timer" | "manual";

export class HeroPager {
  /** The page shown, 0 based. */
  index = 0;
  /** 0 to 1 through the page's dwell, or through its trailer while one plays. */
  progress = 0;
  /** Paged by hand: auto-advance stops for the session (WCAG 2.2.2). */
  stopped = false;

  constructor(
    public count: number,
    public dwellMs: number,
  ) {}

  /** Advances the clock by `dt` ms while auto-advance may run; a page change comes back as its index. */
  tick(dt: number, running: boolean): number | null {
    if (!running || this.stopped || this.count < 2) return null;
    this.progress = Math.min(1, this.progress + dt / this.dwellMs);
    return this.progress >= 1 ? this.go((this.index + 1) % this.count, "timer") : null;
  }

  /** A trailer's place in its cap, written to the same value. */
  trailer(fraction: number) {
    this.progress = Math.max(0, Math.min(1, fraction));
  }

  /** Any key, pointer move or touch: the dwell starts over. */
  input() {
    this.progress = 0;
  }

  go(index: number, why: PagerWhy): number {
    if (why === "manual") this.stopped = true;
    this.index = Math.max(0, Math.min(this.count - 1, index));
    this.progress = 0;
    return this.index;
  }

  /** The list grew or shrank: the page shown stays the same title where it can (`at`, its new index). */
  resize(count: number, at: number) {
    this.count = count;
    this.index = Math.max(0, Math.min(count - 1, at));
  }
}

/** The clock's step for one frame, at most a quarter second: a hidden tab runs no frames, and the first one
 * back must not spend the whole time away on the dwell and page at once (review r1, W7). */
export function frameStep(now: number, last: number): number {
  return Math.max(0, Math.min(now - last, 250));
}

/** CSS's cubic-bezier as a function of time, for the slides the timer drives (the design's ease). */
export function bezier(x1: number, y1: number, x2: number, y2: number): (t: number) => number {
  const at = (a: number, b: number, t: number) => 3 * a * t * (1 - t) ** 2 + 3 * b * t * t * (1 - t) + t ** 3;
  return (x) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    // Bisection on x(t): monotonic for control points inside [0, 1].
    let lo = 0;
    let hi = 1;
    for (let i = 0; i < 30; i++) {
      const mid = (lo + hi) / 2;
      if (at(x1, x2, mid) < x) lo = mid;
      else hi = mid;
    }
    return at(y1, y2, (lo + hi) / 2);
  };
}
