/** The session's profile, for the app's life (plans/m11-profiles.md, P1 apps
 * A, B and v2 3 to 5): one `profiles` view the core keeps open across switches
 * and locks, the teardown a switch needs, and the switch itself. */

import { createEffect, createRoot, createSignal, on } from "solid-js";
import { forgetFocus } from "../focus/dom";
import { startsOnPicker } from "../lib/profiles";
import { BridgeError } from "./bridge";
import { closeProfileViews, liveView, op } from "./client";
import type { ProfilesSnapshot } from "./generated/ProfilesSnapshot";

const view = createRoot(() => liveView<ProfilesSnapshot>("profiles", {}, true));

/** The picker's snapshot, or null until the core sent it (nothing profile-bound opens before). */
export const profiles = () => view.state.data;
export const profilesFailed = () => (view.state.status.state === "failed" ? view.state.status.reason : null);

/** The start picker was answered this launch (a cold start with more than one profile asks once).
 * A reload in the same tab is not a cold start: the tab remembers which profile it chose. */
const [chosen, setChosen] = createSignal(false);
const CHOSEN = "mq.profileChosen";
function choose(active: string | undefined) {
  setChosen(true);
  try {
    if (active) sessionStorage.setItem(CHOSEN, active);
  } catch {
    // Remembered for this page only.
  }
}
const rememberedChoice = () => {
  try {
    return sessionStorage.getItem(CHOSEN);
  } catch {
    return null;
  }
};
export const markChosen = () => choose(profiles()?.active);

/** Whether the app may show profile content, or only the picker. */
export function gateOpen(): boolean {
  const s = profiles();
  if (!s) return false;
  return !s.locked && (chosen() || !startsOnPicker(s));
}

const resets = new Set<() => void | Promise<void>>();
/** Module state a screen keeps outside its views (recent searches, the Trakt view) registers its
 * reset here; a reset that closes a view returns once it closed. */
export function onProfileReset(fn: () => void | Promise<void>) {
  resets.add(fn);
}

/** Everything the old profile left outside the core: views kept for the app's life, focus memory, caches. */
export async function resetProfileState() {
  forgetFocus();
  await Promise.all([...[...resets].map((fn) => fn()), closeProfileViews()]);
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

let switching = false;

/** Tears down, then switches; a view still closing (`viewsOpen`) is waited for, up to five times.
 * One switch at a time: a second one while the first is out is refused. */
export async function switchProfile(id: string, pin?: string): Promise<ProfilesSnapshot> {
  if (switching) throw new Error("A switch is already under way.");
  switching = true;
  try {
    return await runSwitch(id, pin);
  } finally {
    switching = false;
  }
}

async function runSwitch(id: string, pin?: string): Promise<ProfilesSnapshot> {
  await resetProfileState();
  for (let attempt = 1; ; attempt++) {
    try {
      const s = await op<ProfilesSnapshot>("switchProfile", { profile: id, pin });
      choose(s.active);
      return s;
    } catch (e) {
      if (e instanceof BridgeError && e.code === "viewsOpen" && attempt < 5) {
        await sleep(100);
        continue;
      }
      if (e instanceof BridgeError && e.code === "viewsOpen") console.warn("switch: views still open", e.views);
      throw e;
    }
  }
}

/** Unlocks the locked session as its own profile. */
export async function unlockProfile(pin?: string): Promise<ProfilesSnapshot> {
  const s = await op<ProfilesSnapshot>("unlockProfile", { pin });
  choose(s.active);
  return s;
}

// The start picker is a cold start's question only: decided on the first
// snapshot, so a second profile added later never sends the session back to it.
createRoot(() => {
  let decided = false;
  createEffect(() => {
    const s = profiles();
    if (decided || !s) return;
    decided = true;
    if (!startsOnPicker(s) || rememberedChoice() === s.active) setChosen(true);
  });
});

// A lock while the app runs (the active profile removed elsewhere, or locked
// on purpose): the core closed every view but the picker; the app drops what
// it kept for them.
createRoot(() =>
  createEffect(
    on(
      () => profiles()?.locked ?? false,
      (locked, was) => {
        if (locked && was === false) void resetProfileState();
      },
    ),
  ),
);
