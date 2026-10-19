/** Trakt on the web (plans/m7-trakt.md section 3): the words for each state
 * and the records the core hands over, kept apart from the screens so both
 * can be tested. The words come from `apps/shared/trakt-words.json`, the same
 * file the apps read, with `{name}` fields. */

import shared from "../../../shared/trakt-words.json";
import type { TraktLoginSnapshot } from "../core/generated/TraktLoginSnapshot";
import type { TraktPlayback } from "../core/generated/TraktPlayback";
import type { TraktReport } from "../core/generated/TraktReport";
import type { TraktStatus } from "../core/generated/TraktStatus";

/** The words the apps share, as they are, and the web's own: where the sign-in is kept, and
 * the lines a browser shows while it waits. */
export const traktWords = {
  ...shared,
  loading: "Reading Trakt's state",
  waiting: "Waiting for you to approve Marquee on Trakt.",
  signingOut: "Signing out",
  keptInTab: "The sign-in lasts until this tab closes or reloads. With sync on, Remember this browser on Devices and sync keeps it.",
  keptSealed: "The sign-in is kept in this browser, locked with your passkey.",
} as const;

/** Fills `{name}` fields. */
export const fill = (line: string, fields: Record<string, string | number>) =>
  line.replace(/\{(\w+)\}/g, (all, name: string) => (name in fields ? String(fields[name]) : all));

/** "4:05" left of the code, never below zero. */
export function timeLeft(expiresMs: number, nowMs: number): string {
  const s = Math.max(0, Math.ceil((expiresMs - nowMs) / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** What the sign-in step says, by phase (the code step has its own layout). */
export function loginLine(login: TraktLoginSnapshot): string {
  switch (login.phase) {
    case "starting":
      return traktWords.starting;
    case "code":
      return traktWords.waiting;
    case "done":
      return fill(traktWords.connected, { user: login.user });
    case "expired":
      return traktWords.expired;
    case "denied":
      return traktWords.denied;
    case "notConfigured":
      return traktWords.notConfigured;
    case "failed":
      return fill(traktWords.failed, { reason: login.reason });
  }
}

/** A sign-in that ended without connecting offers a new code (never a build without Trakt). */
export const loginRetry = (login: TraktLoginSnapshot) => login.phase === "expired" || login.phase === "denied" || login.phase === "failed";

export type TraktAction = "connect" | "signInHere" | "signInAgain" | "syncNow" | "signOut";

/** The card's line and its actions for a state. `clock` formats a time of day. */
export function stateCard(status: TraktStatus, clock: (ms: number) => string): { line: string; detail?: string; actions: TraktAction[] } {
  switch (status.state) {
    case "connected":
      return { line: status.user ? fill(traktWords.connected, { user: status.user }) : traktWords.connectedNoUser, actions: ["syncNow", "signOut"] };
    case "failing":
      return {
        line: fill(traktWords.failing, { time: clock(status.sinceMs), reason: status.reason }),
        ...(status.lastError ? { detail: fill(traktWords.lastError, { error: status.lastError }) } : {}),
        actions: ["syncNow", "signOut"],
      };
    case "reconnect":
      return { line: fill(traktWords.reconnect, { reason: status.reason }), actions: ["signInAgain", "signOut"] };
    case "off":
      return { line: traktWords.off, actions: ["connect"] };
    case "thisDeviceOff":
      return { line: traktWords.thisDeviceOff, actions: ["signInHere"] };
    case "notConfigured":
      return { line: traktWords.notConfigured, actions: [] };
  }
}

/** Writes still waiting, in words, or nothing. */
export const pendingLine = (count: number) =>
  count <= 0 ? null : count === 1 ? traktWords.pendingOne : fill(traktWords.pendingMany, { count });

export const syncedLine = (r: TraktReport) => fill(traktWords.synced, { pushed: r.pushed, applied: r.applied });

/** The player's mark: none when the profile does not use Trakt. */
export function playbackMark(t: TraktPlayback | undefined): { tone: "on" | "warn"; label: string } | null {
  switch (t) {
    case "on":
      return { tone: "on", label: traktWords.playerOn };
    case "unmapped":
      return { tone: "warn", label: traktWords.playerUnmapped };
    case "thisDeviceOff":
      return { tone: "warn", label: traktWords.playerThisDeviceOff };
    default:
      return null;
  }
}

/** The record the core hands over (`TraktRecord`, camelCase on the wire). */
export interface TraktRecord {
  accessToken: string;
  refreshToken: string;
  createdAtMs: number;
  expiresAtMs: number;
  scope: string;
  account: string;
  user: string;
  revision: number;
  generation: number;
}

/** The newer of two records: a later connection, then a later rotation. */
export function newer(a: TraktRecord | undefined, b: TraktRecord): TraktRecord {
  if (!a) return b;
  return b.generation > a.generation || (b.generation === a.generation && b.revision > a.revision) ? b : a;
}

export interface TraktRecordsIo {
  /** `setTrakt`: the acknowledgement of an offered record, and the resend after open, unlock and a switch. */
  send(record: TraktRecord | null): Promise<void>;
  /** Keeps every profile's record where this browser remembers (resolves at once where it does not). */
  persist(all: Record<string, TraktRecord>): Promise<void>;
}

/** Every profile's Trakt record, in this tab's memory (and through `persist`, sealed where
 * the browser remembers). Each change runs in turn, in the order it came. */
export class TraktRecords {
  private all: Record<string, TraktRecord> = {};
  private queue: Promise<unknown> = Promise.resolve();

  constructor(private readonly io: TraktRecordsIo) {}

  private inTurn<T>(work: () => Promise<T>): Promise<T> {
    const run = this.queue.then(work, work);
    this.queue = run.catch(() => undefined);
    return run;
  }

  private persist() {
    // Memory holds it either way: a storage that fails says so itself and never holds back the acknowledgement.
    return this.io.persist({ ...this.all }).catch(() => undefined);
  }

  get(profile: string): TraktRecord | undefined {
    return this.all[profile];
  }

  /** A record the `trakt` view offered for `profile`: stored first, then acknowledged. */
  offered(profile: string, record: TraktRecord): Promise<void> {
    return this.inTurn(async () => {
      this.all[profile] = record;
      await this.persist();
      await this.io.send(record);
    });
  }

  /** After the core opened, an unlock or a switch: this profile's record, or none. */
  resend(profile: string): Promise<void> {
    return this.inTurn(() => this.io.send(this.all[profile] ?? null));
  }

  /** Signed out of Trakt: the profile's record goes, here and where it was sealed. */
  signedOut(profile: string): Promise<void> {
    return this.inTurn(async () => {
      delete this.all[profile];
      await this.persist();
    });
  }

  /** What the browser had sealed, now opened (or a Remember just turned on): the newer of each
   * profile's two is kept and sealed again; true when `profile`'s record changed. */
  restored(sealed: Record<string, TraktRecord>, profile: string): Promise<boolean> {
    return this.inTurn(async () => {
      const before = this.all[profile];
      for (const [id, record] of Object.entries(sealed)) this.all[id] = newer(this.all[id], record);
      await this.persist();
      return this.all[profile] !== before;
    });
  }
}
