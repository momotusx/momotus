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
  waiting: "Waiting for you to approve Momotus on Trakt.",
  signingOut: "Signing out",
  keptInTab: "The sign-in lasts until this tab closes or reloads. With sync on, Remember this browser on the Account page keeps it.",
  keptSealed: "The sign-in is kept in this browser, locked with your passkey.",
  unlockToUse: "Unlock to use Trakt",
  lockedLine: "This browser keeps this profile's Trakt sign-in, locked with your passkey. Unlock it on the Account page to use Trakt here.",
  saveFailed: "This browser could not store the newest Trakt sign-in. Until it does, a reload may ask you to sign in to Trakt again.",
  saveAgain: "Save again",
  deleteFailed: "Signed out of Trakt, but this browser could not delete the sign-in it keeps. Try again.",
  readFailed: "This browser could not read the Trakt sign-ins it keeps, so it stores no new one over them. Try again.",
  tryAgain: "Try again",
  plainLink: "Trakt is off on this page: its link to Momotus's core is not encrypted, so no Trakt sign-in may travel over it. Open Momotus on the computer that runs the core, or use the app.",
  backToSettings: "Back to Settings",
  importLabel: "Trakt history brought in",
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

export type TraktAction = "connect" | "signInHere" | "signInAgain" | "syncNow" | "signOut" | "unlock";

/** The card's line and its actions for a state. `clock` formats a time of day. `lockedHere`: this
 * browser keeps the profile's sign-in under a passkey not opened yet, so unlocking brings it back. */
export function stateCard(status: TraktStatus, clock: (ms: number) => string, lockedHere = false): { line: string; detail?: string; actions: TraktAction[] } {
  if (lockedHere && (status.state === "off" || status.state === "thisDeviceOff")) return { line: traktWords.lockedLine, actions: ["unlock"] };
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

/** Settings' line for Trakt: off (or not read yet) invites, the other states say themselves. */
export function rowLine(status: TraktStatus | null, privateLink: boolean, clock: (ms: number) => string, storage: string | null = null, lockedHere = false): string {
  if (!privateLink) return traktWords.plainLink;
  if (storage) return storage;
  if (status && lockedHere) return stateCard(status, clock, true).line;
  if (!status || status.state === "off") return traktWords.rowOff;
  return stateCard(status, clock).line;
}

/** Writes still waiting, in words, or nothing. */
export const pendingLine = (count: number) =>
  count <= 0 ? null : count === 1 ? traktWords.pendingOne : fill(traktWords.pendingMany, { count });

export const syncedLine = (r: TraktReport) => fill(traktWords.synced, { pushed: r.pushed, applied: r.applied });

/** How long the player's Trakt notice shows, once per play (owner, 2026-11-08: it must never stay on the video). */
export const NOTICE_MS = 4000;

/** When this play's notice first showed, or null before it did. Only `reconnect` starts it (plans/player-bar.md D9:
 * the one state where progress silently stops being recorded); the other states show nothing in the player. Once
 * set it stays for the play, so a pause, a seek, a resume or a later state never shows it again; Settings, Trakt
 * keeps the state and its Sign in again. */
export const noticeSince = (since: number | null, t: TraktPlayback | undefined, now: number): number | null =>
  since ?? (t === "reconnect" ? now : null);

/** Whether the notice shows at `now`: only in its first `NOTICE_MS`. */
export const noticeShows = (since: number | null, now: number): boolean =>
  since !== null && now >= since && now - since < NOTICE_MS;

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
  /** `setTrakt`: the acknowledgement of an offered record, and the resend before the view opens. */
  send(record: TraktRecord): Promise<void>;
  /** Keeps every profile's record where this browser remembers (resolves at once where it does
   * not); rejects when that storage failed. */
  persist(all: Record<string, TraktRecord>): Promise<void>;
  /** Deletes the profile's kept record, whether or not this browser's lock is open; rejects when
   * the storage failed. */
  forget(profile: string): Promise<void>;
}

/** Every profile's Trakt record, in this tab's memory (and through `persist`, sealed where the
 * browser remembers). Each change runs in turn, in the order it came. Nothing is ever sent for
 * a profile once `still()` says its session is over: a switch or a lock in between drops it. */
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
    return this.io.persist({ ...this.all });
  }

  get(profile: string): TraktRecord | undefined {
    return this.all[profile];
  }

  /** A record the `trakt` view offered for `profile`: kept in memory, stored, then acknowledged
   * while `still()` holds. A store that fails rejects and acknowledges nothing: the core offers
   * the record again, and `save` retries. */
  offered(profile: string, record: TraktRecord, still: () => boolean): Promise<void> {
    return this.inTurn(async () => {
      this.all[profile] = record;
      await this.persist();
      if (still()) await this.io.send(record);
    });
  }

  /** Before the view opens (at start, after a switch or an unlock): this profile's record, while
   * `still()` holds. A page holding none sends nothing: the core keeps what another tab gave it. */
  resend(profile: string, still: () => boolean): Promise<void> {
    return this.inTurn(async () => {
      const record = this.all[profile];
      if (record && still()) await this.io.send(record);
    });
  }

  /** Signed out of Trakt: the profile's record goes, here and where it was sealed, locked or
   * not (rejects when the sealed copy could not be deleted; calling it again tries again). */
  signedOut(profile: string): Promise<void> {
    return this.inTurn(async () => {
      delete this.all[profile];
      await this.io.forget(profile);
    });
  }

  /** Stores what memory holds again, after a store that failed. */
  save(): Promise<void> {
    return this.inTurn(() => this.persist());
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
