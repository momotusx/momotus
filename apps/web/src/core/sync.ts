/** This device's sync account (plans/core-protocol.md, the sync ops). A
 * browser has no keystore that can hold the record safely, so it lives in this
 * tab's memory, as the debrid records do. Every op that takes the record runs
 * in turn, and a record an answer carries is stored before the next one runs. */

import { createSignal } from "solid-js";
import { keepNewer, type SyncRecord } from "../lib/sync";
import { plainAddress, syncProblem } from "../lib/words";
import { form } from "../form";
import { BridgeError } from "./bridge";
import { op } from "./client";
import type { Devices } from "./generated/Devices";
import type { LostPreview } from "./generated/LostPreview";
import type { SyncReport } from "./generated/SyncReport";

let record: SyncRecord | null = null;
/** Bumped by every join and sign-out: an answer from before it stores nothing. */
let account = 0;

const [joined, setJoined] = createSignal(false);
const [report, setReport] = createSignal<SyncReport | null>(null);
const [syncedAt, setSyncedAt] = createSignal<number | null>(null);
const [syncing, setSyncing] = createSignal(false);
const [problem, setProblem] = createSignal<{ code?: string; words: string } | null>(null);
const [devices, setDevices] = createSignal<Devices | null>(null);
/** Joined with a recovery kit and no new kit made since: the screen asks for one. */
const [recovered, setRecovered] = createSignal(false);
export { devices, joined, problem, recovered, report, syncedAt, syncing };

export const problemOf = (e: unknown) => ({
  code: e instanceof BridgeError ? e.code : undefined,
  words: syncProblem(e instanceof BridgeError ? e.code : undefined, e instanceof Error ? e.message : String(e)),
});

let queue: Promise<unknown> = Promise.resolve();
function inTurn<T>(work: () => Promise<T>): Promise<T> {
  const run = queue.then(work, work);
  queue = run.catch(() => undefined);
  return run;
}

/** Runs an op on the record, in turn. A record in the answer is stored at once;
 * `dropped` (this device was removed) forgets the account here. */
function withRecord<T>(name: string, fields: Record<string, unknown>, recordIn: (result: T) => SyncRecord | null | undefined): Promise<T> {
  return inTurn(async () => {
    const mine = account;
    if (!record) throw new BridgeError("this device has not joined the account", "notJoined");
    try {
      const result = await op<T>(name, { ...fields, record });
      const next = recordIn(result);
      if (next && mine === account) record = keepNewer(record, next);
      return result;
    } catch (e) {
      if (mine === account && e instanceof BridgeError && (e.code === "dropped" || e.code === "fork")) {
        if (e.code === "dropped") forget();
        setProblem(problemOf(e));
      }
      throw e;
    }
  });
}

function forget() {
  account++;
  record = null;
  setJoined(false);
  setReport(null);
  setSyncedAt(null);
  setDevices(null);
  setRecovered(false);
}

/** A new account record (created, paired or recovered) replaces any before it; its first round runs at once. */
export function join(next: SyncRecord, recoveredWithKit = false) {
  forget();
  record = next;
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
      await rounds();
    }
    setSyncing(false);
    running = null;
  })();
  return running;
}

/** A round that changed the record publishes nothing until it is stored:
 * go again while the core asks to. */
async function rounds() {
  for (let i = 0; i < 5 && record; i++) {
    const mine = account;
    try {
      const r = await withRecord<SyncReport>("syncNow", {}, (x) => x.record);
      if (mine !== account) return;
      setReport(r);
      setSyncedAt(Date.now());
      setProblem(null);
      if (!r.again) return;
    } catch (e) {
      if (mine === account || !record) setProblem(problemOf(e));
      return;
    }
  }
}

// ponytail: a round a minute while the tab is visible and on coming back to it; the apps
// sync a few seconds after each edit, add that here when the web gets an edit signal.
setInterval(() => joined() && !document.hidden && void syncNow(), 60_000);
document.addEventListener("visibilitychange", () => joined() && !document.hidden && void syncNow());

export async function loadDevices(): Promise<Devices> {
  const list = await withRecord<Devices>("syncDevices", {}, (d) => d.record);
  setDevices(list);
  return list;
}

export const createAccount = async (server: string, name: string, invite: string) =>
  join(await op<SyncRecord>("createSyncAccount", { server, name, ...(invite ? { invite } : {}) }));

export const recover = async (server: string, kit: string, name: string) => join(await op<SyncRecord>("recover", { server, kit, name }), true);

/** Vouches for the device that shows `qr`; returns the six digits it must show too. */
export const approvePairing = async (qr: string) => (await withRecord<{ code: string; record: SyncRecord }>("approvePairing", { qr }, (r) => r.record)).code;

/** The kit, to show once: nothing here keeps it. */
export async function makeKit(): Promise<string> {
  const made = await withRecord<{ kit: string; record: SyncRecord }>("createRecoveryKit", {}, (r) => r.record);
  setRecovered(false);
  return made.kit;
}

export const lostPreview = (lost: string) => withRecord<LostPreview>("lostPreview", { lost }, () => null);

export async function removeDevice(device: string) {
  await withRecord<SyncRecord>("removeDevice", { device }, (r) => r);
  await loadDevices();
}

export async function reportLost(lost: string, keep: string[]) {
  await withRecord<SyncRecord>("reportLost", { lost, keep }, (r) => r);
  await loadDevices();
}

/** Leaves the account; the library stays. Without `force` the core refuses while edits are unsent (`unsent`). */
export async function signOut(force: boolean) {
  await withRecord("signOut", { force }, () => null);
  forget();
  setProblem(null);
}

/** After `fork`: the server's device list wins over this device's. */
export async function useServerRoster() {
  await withRecord<SyncRecord>("useServerRoster", { drop: [] }, (r) => r);
  setProblem(null);
  await syncNow();
}

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
export function remember(server: string, name: string) {
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
