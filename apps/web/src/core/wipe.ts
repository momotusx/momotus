/** Sign out with removal (D13), after the leave or on a device whose account was deleted: the
 * keystore's sync record is gone already (core/sync.ts forgets it on leaving); every profile is
 * signed out of Trakt here and each record deleted, each debrid account disconnected tombstone
 * first when asked, then the wipe, which the core refuses while either is left. A mark kept
 * until the wipe answered makes the next start finish it, so no crash leaves the previous
 * owner's debrid key or Trakt sign-in to the next person. */

import { createEffect, createRoot, createSignal } from "solid-js";
import { accountWords, activeProfile } from "../lib/account";
import { op } from "./client";
import { debridAccounts, debridStore } from "./debrid";
import { gateOpen, profiles } from "./profiles";
import { clearProblem, joined, problemOf } from "./sync";
import { traktSignOutEvery } from "./trakt";

const KEY = "mq.wipePending";
/** A removal ran on this page already: the start does not run it again. */
let resumed = false;

/** The removal this browser started and has not finished: with or without the debrid accounts. */
export function pendingWipe(): { debrid: boolean } | null {
  try {
    const v = localStorage.getItem(KEY);
    return v === null ? null : { debrid: v === "debrid" };
  } catch {
    return null;
  }
}

function mark(debrid: boolean | null) {
  try {
    if (debrid === null) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, debrid ? "debrid" : "keep");
  } catch {
    // This page finishes it or says it did not.
  }
}

/** Running now, or why the last run stopped (the page offers Try again). */
const [wipe, setWipe] = createSignal<{ running: boolean; problem: string | null }>({ running: false, problem: null });
export { wipe as wipeState };

/** Steps two and three; the page then starts as new. The owner profile's (`notOwner` otherwise). */
export async function removeHere(debrid: boolean) {
  if (wipe().running) return;
  resumed = true;
  mark(debrid);
  setWipe({ running: true, problem: null });
  try {
    await traktSignOutEvery(profiles()?.profiles.map((p) => p.id) ?? []);
    // ponytail: polls the debrid list the app opens at start (a few seconds at most), no signal wiring.
    for (let i = 0; i < 100 && debrid && debridAccounts() === null; i++) await new Promise((ok) => setTimeout(ok, 100));
    if (debrid) for (const a of debridAccounts()?.accounts ?? []) await debridStore.disconnect(a.service, a.generation);
    await op("signOut", { wipe: true, disconnectDebrid: debrid });
  } catch (e) {
    const code = problemOf(e).code;
    const said = code === "debridConnected" || code === "traktConnected" || code === "notOwner" ? accountWords[code] : accountWords.wipeFailed;
    setWipe({ running: false, problem: said });
    throw e;
  }
  mark(null);
  clearProblem();
  location.replace("/");
}

// At start, on the owner profile of a device that holds no account: a removal a reload or a
// crash cut short goes on, once per page.
createRoot(() =>
  createEffect(() => {
    const pending = pendingWipe();
    if (resumed || !pending || !gateOpen() || joined() || !activeProfile(profiles())?.isOwner) return;
    resumed = true;
    void removeHere(pending.debrid).catch(() => undefined);
  }),
);
