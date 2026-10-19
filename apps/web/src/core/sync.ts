/** This device's sync account (plans/core-protocol.md, the sync ops). The
 * record lives in this tab's memory; with "Remember this browser" it is also
 * sealed under a passkey (src/core/vault.ts). Every op that takes the record
 * runs in turn, and a record an answer carries is stored before the next one
 * runs. Nothing here logs, and no record travels a link that is not private. */

import { createSignal } from "solid-js";
import { form } from "../form";
import { keepNewer, type SyncRecord } from "../lib/sync";
import { outcomeOf, plainAddress, syncProblem, syncWords } from "../lib/words";
import { BridgeError } from "./bridge";
import { op, privateLink } from "./client";
import type { Devices } from "./generated/Devices";
import type { ForkPreview } from "./generated/ForkPreview";
import type { LostPreview } from "./generated/LostPreview";
import type { SyncReport } from "./generated/SyncReport";
import * as vault from "./vault";

let record: SyncRecord | null = null;
/** Bumped by every join and sign-out: an answer from before it stores nothing. */
let account = 0;

const [joined, setJoined] = createSignal(false);
const [report, setReport] = createSignal<SyncReport | null>(null);
const [syncedAt, setSyncedAt] = createSignal<number | null>(null);
const [syncing, setSyncing] = createSignal(false);
/** Rounds finished since the page opened, whatever their outcome (for "this round is done"). */
const [rounds, setRounds] = createSignal(0);
const [problem, setProblem] = createSignal<{ code?: string; words: string } | null>(null);
/** What the last action here did, when it ended the account on purpose ("no longer syncs"). */
const [notice, setNotice] = createSignal<string | null>(null);
const [devices, setDevices] = createSignal<Devices | null>(null);
/** The 12 characters from whichever answer came last: a round or the device list. */
const [fingerprint, setFingerprint] = createSignal<string | null>(null);
/** Joined with a recovery kit and no new kit made since: the screen asks for one. */
const [recovered, setRecovered] = createSignal(false);
/** Changes that failed since this device joined, until the user dismisses them. */
const [failed, setFailed] = createSignal({ refused: 0, unreadable: 0, newer: 0 });
/** This browser remembers an account that the passkey has not opened yet. */
const [locked, setLocked] = createSignal(false);
const [remembered, setRemembered] = createSignal(false);
/** Why this browser stopped remembering the account (a write that did not commit). */
const [memoryProblem, setMemoryProblem] = createSignal<string | null>(null);
/** Whether this browser was checked for a remembered account yet: until then nothing may start a new one. */
const [settled, setSettled] = createSignal(false);
/** Checks of this browser's storage that failed in a row (0: it was read). */
const [unreadable, setUnreadable] = createSignal(0);
/** The user went on without this browser's storage: nothing is read or sealed here until a reload. */
const [memoryOnly, setMemoryOnly] = createSignal(false);
/** Bumped by Forget and by leaving: a passkey answer from before it changes nothing. */
let vaultGen = 0;
export { devices, failed, fingerprint, joined, locked, memoryOnly, memoryProblem, notice, problem, recovered, remembered, report, rounds, settled, syncedAt, syncing, unreadable };

export const dismissFailed = () => setFailed({ refused: 0, unreadable: 0, newer: 0 });

const code = (e: unknown) => (e instanceof BridgeError || e instanceof vault.VaultError ? e.code : undefined);
export const problemOf = (e: unknown) => ({ code: code(e), words: syncProblem(code(e), e instanceof Error ? e.message : String(e)) });

/** The record never travels a link that is neither encrypted nor on this machine. */
function guardLink() {
  if (!privateLink) throw new BridgeError(syncWords.plainLink, "plainLink");
}

let queue: Promise<unknown> = Promise.resolve();
function inTurn<T>(work: () => Promise<T>): Promise<T> {
  const run = queue.then(work, work);
  queue = run.catch(() => undefined);
  return run;
}

/** Keeps a newer record, and seals it before the op that brought it returns. */
async function store(next: SyncRecord) {
  record = keepNewer(record, next);
  if (vault.active()) await vault.save(record).then(savedNewest, saveFailed);
}

/** A sealed write that did not commit. The copy sealed before stays (it is older but still
 * opens this account, and the core brings an older record up to date), and so do the passkey
 * gate and the claim to remember; the user is told where they are, and can save again. */
function saveFailed() {
  setMemoryProblem(syncWords.saveFailed);
}

function savedNewest() {
  if (memoryProblem() === syncWords.saveFailed) setMemoryProblem(null);
}

/** "Save again" after a failed sealed write. */
export async function saveAgain() {
  if (record && vault.active()) await vault.save(record).then(savedNewest, saveFailed);
}

/** Runs an op on the record, in turn. A record in the answer is stored at
 * once; `dropped` (this device was removed) forgets the account here. */
function withRecord<T>(name: string, fields: Record<string, unknown>, recordIn: (result: T) => SyncRecord | null | undefined): Promise<T> {
  return inTurn(async () => {
    guardLink();
    const mine = account;
    if (!record) throw new BridgeError("this device has not joined the account", "notJoined");
    try {
      const result = await op<T>(name, { ...fields, record });
      const next = recordIn(result);
      if (next && mine === account) await store(next);
      return result;
    } catch (e) {
      if (mine === account && e instanceof BridgeError && (e.code === "dropped" || e.code === "fork")) {
        if (e.code === "dropped") await forget();
        setProblem(problemOf(e));
      }
      throw e;
    }
  });
}

/** Drops the account from this page's memory; what this browser remembers stays. */
function reset() {
  account++;
  record = null;
  setJoined(false);
  setReport(null);
  setSyncedAt(null);
  setDevices(null);
  setFingerprint(null);
  setRecovered(false);
  setFailed({ refused: 0, unreadable: 0, newer: 0 });
}

/** The account is gone here (left, or this device was removed): its sealed copy goes too, before
 * the caller says so. A copy that stays is said where the user is, with Forget. Only this
 * account's own copy, the one this page holds the key to: a page that never remembered it
 * (memory only, or another account's copy here) touches no storage. */
async function forget() {
  const sealedHere = vault.active();
  reset();
  vaultGen++;
  setLocked(false);
  setRemembered(false);
  if (!sealedHere) return;
  try {
    await vault.clear();
    setMemoryProblem(null);
  } catch {
    vault.drop();
    setMemoryProblem(syncWords.leftKept);
  }
}

/** A new account record replaces none: a device that syncs (or remembers an
 * account) leaves first, so an account never keeps a ghost of this browser. */
function refuseSecond() {
  if (record || locked()) throw new BridgeError(syncWords.alreadyJoined, "alreadyJoined");
}

/** A new account record (created, paired or recovered); its first round runs at once. */
export function join(next: SyncRecord, recoveredWithKit = false) {
  reset();
  record = next;
  setNotice(null);
  // Opened from this browser's passkey: it stays remembered.
  setRemembered(vault.active());
  setJoined(true);
  setProblem(null);
  setRecovered(recoveredWithKit);
  void syncNow();
}

let running: Promise<void> | null = null;
let more = false;

/** Asks for a round: now, or once the running one ends. */
export function syncNow(): Promise<void> {
  more = true;
  running ??= (async () => {
    setSyncing(true);
    while (more) {
      more = false;
      await roundsOnce();
    }
    setSyncing(false);
    running = null;
  })();
  return running;
}

/** A round that changed the record publishes nothing until it is stored:
 * go again while the core asks to (at most five times; the last report then
 * says it stopped before it finished). */
async function roundsOnce() {
  try {
    for (let i = 0; i < 5 && record; i++) {
      const mine = account;
      try {
        const r = await withRecord<SyncReport>("syncNow", {}, (x) => x.record);
        if (mine !== account) return;
        setReport(r);
        setFingerprint(r.fingerprint);
        setSyncedAt(Date.now());
        setProblem(null);
        if (r.refused || r.unreadable || r.newer)
          setFailed((f) => ({ refused: f.refused + r.refused, unreadable: f.unreadable + r.unreadable, newer: f.newer + r.newer }));
        if (!r.again) return;
      } catch (e) {
        // A removal (dropped) or a different list (fork) was said by withRecord already.
        if (mine === account) setProblem(problemOf(e));
        return;
      }
    }
  } finally {
    setRounds((n) => n + 1);
  }
}

// ponytail: a round a minute while the tab is visible and on coming back to it; the apps
// sync a few seconds after each edit, add that here when the web gets an edit signal.
setInterval(() => joined() && !document.hidden && void syncNow(), 60_000);
document.addEventListener("visibilitychange", () => joined() && !document.hidden && void syncNow());

/** The device list, always read again from the sync server. */
export async function loadDevices(): Promise<Devices> {
  const list = await withRecord<Devices>("syncDevices", {}, (d) => d.record);
  setDevices(list);
  setFingerprint(list.fingerprint);
  return list;
}

export async function createAccount(server: string, name: string, invite: string) {
  await vaultReady;
  refuseSecond();
  guardLink();
  join(await op<SyncRecord>("createSyncAccount", { server, name, ...(invite ? { invite } : {}) }));
}

export async function recover(server: string, kit: string, name: string) {
  await vaultReady;
  refuseSecond();
  guardLink();
  join(await op<SyncRecord>("recover", { server, kit, name }), true);
}

/** Pairing may start here: no account syncs or waits to be unlocked, and the link is private. Ask after `vaultReady`. */
export const mayPair = () => !record && !locked() && privateLink;

/** A pairing that finished on this page: refused when an account appeared meanwhile. */
export function joinPaired(next: SyncRecord) {
  refuseSecond();
  join(next);
}

/** Before a change that cannot be undone: the passkey when this browser remembers the account. */
async function gate() {
  if (vault.active()) await vault.verify();
}

/** Vouches for the device that shows `qr`; returns the six digits it must show too. */
/** Vouches for the device that shows `qr`; returns the six digits it must show too. Hands over the account, so the passkey first. */
export async function approvePairing(qr: string): Promise<string> {
  await gate();
  return (await withRecord<{ code: string; record: SyncRecord }>("approvePairing", { qr }, (r) => r.record)).code;
}

/** The digits on the new device differ: the approval of `qr` ends now, with a key rotation
 * that drops the device if it joined. The passkey first. */
export async function cancelPairing(qr: string) {
  await gate();
  await withRecord<SyncRecord>("cancelPairing", { qr }, (r) => r);
  setFingerprint(null);
}

/** The kit, to show once: nothing here keeps it. */
export async function makeKit(): Promise<string> {
  await gate();
  const made = await withRecord<{ kit: string; record: SyncRecord }>("createRecoveryKit", {}, (r) => r.record);
  setRecovered(false);
  setFingerprint(null);
  return made.kit;
}

export const lostPreview = (lost: string) => withRecord<LostPreview>("lostPreview", { lost }, () => null);

/** Removes a device. The list is not read again here: a failed refresh never turns a done removal into a failure. */
export async function removeDevice(device: string) {
  await gate();
  await withRecord<SyncRecord>("removeDevice", { device }, (r) => r);
  setFingerprint(null);
}

export async function reportLost(lost: string, keep: string[]) {
  await gate();
  await withRecord<SyncRecord>("reportLost", { lost, keep }, (r) => r);
  setFingerprint(null);
}

/** Leaves the account; the library stays. Without `force` the core refuses while edits are unsent (`unsent`). */
export async function signOut(force: boolean, local = false) {
  const mine = account;
  await gate();
  // `local`: nothing sent; only this device forgets the account (the way out of a list it is not on).
  await withRecord("signOut", { force, local }, () => null);
  if (mine !== account) return;
  await forget();
  setProblem(null);
  setNotice(local ? syncWords.leftHere : syncWords.left);
}

/** Both sides of a different device list, read only. */
export const forkPreview = () => withRecord<ForkPreview>("forkPreview", {}, () => null);

/** After `fork`: the server's device list wins, dropping what it drops on its own and `drop`.
 * `head` is the preview's: a list that moved since is refused (`rosterChanged`). */
export async function useServerRoster(drop: string[], head: string) {
  await gate();
  await withRecord<SyncRecord>("useServerRoster", { drop, head }, (r) => r);
  setProblem(null);
  setFingerprint(null);
  await syncNow();
}

/** Seals the record under a new passkey for this browser. */
export async function remember() {
  const r = record;
  if (!r) throw new BridgeError("this device has not joined the account", "notJoined");
  const gen = vaultGen;
  const mine = account;
  await vault.enable(r);
  // Left or told to forget during the prompt: nothing stays sealed.
  if (gen !== vaultGen || mine !== account) return void (await vault.clear());
  // A record that changed during the prompt replaces the one sealed, and the checks hold after it.
  // The new copy is sealed: this browser remembers, with the newer record if that save works.
  setMemoryProblem(null);
  if (record && record !== r) {
    await vault.save(record).then(savedNewest, saveFailed);
    if (gen !== vaultGen || mine !== account) return void (await vault.clear());
  }
  setRemembered(true);
}

/** Stops remembering: the sealed copy goes; a joined page keeps syncing until it closes. */
export async function forgetBrowser() {
  vaultGen++;
  try {
    await vault.clear();
  } catch (e) {
    // Still sealed here: the page keeps saying so.
    setMemoryProblem(syncWords.forgetFailed);
    throw e;
  }
  setRemembered(false);
  setLocked(false);
  setMemoryProblem(null);
}

/** Opens what this browser remembers, with the passkey, then syncs. */
export async function unlock() {
  const gen = vaultGen;
  const opened = await vault.unlock();
  // Forgotten during the prompt, or an account joined meanwhile: the opened copy is dropped.
  if (gen !== vaultGen || record) return void (await vault.clear());
  setLocked(false);
  join(opened);
  setMemoryProblem(null);
}

/** Checks this browser for a remembered account. A storage that cannot be read may hold one:
 * that counts as locked, with its own words, so nothing new starts over it. */
async function discover() {
  try {
    // A storage that does not answer in time counts as one that could not be read.
    const late = new Promise<never>((_, fail) => setTimeout(() => fail(new Error("the storage did not answer")), 10_000));
    const has = await Promise.race([vault.stored(), late]);
    setUnreadable(0);
    if (!record) {
      setLocked(has);
      setMemoryProblem(null);
    }
  } catch {
    setUnreadable((n) => n + 1);
    if (!record) {
      setLocked(true);
      setMemoryProblem(syncWords.vaultUnreadable);
    }
  }
}

/** After a second failed check: this tab syncs from memory only. Whatever the storage may hold
 * stays there unread, and nothing is sealed into it. */
export function continueWithout() {
  setMemoryOnly(true);
  setLocked(false);
  setMemoryProblem(null);
}

/** Settles once this browser was checked for a remembered account. */
export const vaultReady: Promise<void> = discover().finally(() => setSettled(true));

/** "Check again" after a storage that could not be read. */
export const recheckVault = discover;

const SERVER_KEY = "mq.syncServer";
const NAME_KEY = "mq.syncName";

/** The sync server this device used before, when it is a plain address (an
 * older version may have saved one with a user name or password: not used). */
export function rememberedServer(): string {
  try {
    const s = (localStorage.getItem(SERVER_KEY) ?? "").trim();
    return plainAddress(s) ? s : "";
  } catch {
    return "";
  }
}

/** The server and this device's name, for the next pairing or kit: neither is a secret. */
export function rememberJoin(server: string, name: string) {
  try {
    localStorage.setItem(SERVER_KEY, server);
    localStorage.setItem(NAME_KEY, name);
  } catch {
    // Remembered for this page only.
  }
}

/** How other devices list this one: the name it joined with last, else one by its form. */
export function defaultDeviceName(): string {
  try {
    const n = localStorage.getItem(NAME_KEY)?.trim();
    if (n) return n;
  } catch {
    // Private mode: the form's name.
  }
  return `Marquee ${form() === "tv" ? "TV" : form() === "phone" ? "phone" : "desktop"}`;
}
export const passkeyAvailable = vault.available;
