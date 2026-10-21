/** Sign out with removal (D13), after the leave or on a device whose account was deleted. The
 * core keeps the removal (`signOut {removeAfter}`, recorded before the leave goes out, cleared by a
 * failed leave, a join or the wipe) and reports it in the profiles view as `wipePending`. Steps two
 * and three here: every profile is signed out of Trakt and each record deleted, each debrid account
 * disconnected tombstone first when asked, then the wipe, which the core refuses while either is
 * left. A start that finds the removal pending finishes it, so no crash or closed tab leaves the
 * previous owner's debrid key or Trakt sign-in to the next person. */

import { createEffect, createRoot, createSignal } from "solid-js";
import { accountWords, activeProfile } from "../lib/account";
import { op } from "./client";
import { debridAccounts, debridStore } from "./debrid";
import { gateOpen, profiles } from "./profiles";
import { clearProblem, joined, problemOf } from "./sync";
import { traktSignOutEvery } from "./trakt";

/** The removal the core holds and has not finished: with or without the debrid accounts. */
export const pendingWipe = () => profiles()?.wipePending ?? null;

/** Running now, or why the last run stopped (the page offers Try again). */
const [wipe, setWipe] = createSignal<{ running: boolean; problem: string | null }>({ running: false, problem: null });
export { wipe as wipeState };
/** A removal ran on this page already: the start does not run it again. */
let resumed = false;

/** Steps two and three; the page then starts as new. The owner profile's (`notOwner` otherwise).
 * `record`: a device whose account was deleted has no leave, so the removal is recorded first. */
export async function removeHere(debrid: boolean, record = false) {
  if (wipe().running) return;
  resumed = true;
  setWipe({ running: true, problem: null });
  try {
    if (record) await op("signOut", { removeAfter: { debrid } });
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
  clearProblem();
  location.replace("/");
}

// After a sign-out with removal, and at start: on the owner profile of a device that holds no
// account, a removal the core holds goes on, once each time it is asked.
createRoot(() =>
  createEffect(() => {
    const pending = pendingWipe();
    if (!pending) resumed = false;
    if (resumed || !pending || !gateOpen() || joined() || !activeProfile(profiles())?.isOwner) return;
    void removeHere(pending.debrid).catch(() => undefined);
  }),
);
