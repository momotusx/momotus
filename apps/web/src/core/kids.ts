/** The owner's side of kids profiles (plans/m11-kids.md 6): which titles each kids profile may watch, from the core's
 * live `kidsKeys`, and the policy writes, each followed by a sync round. */

import { createRoot } from "solid-js";
import { managesKids, writePolicy } from "../lib/kids";
import { liveView, type LiveView, op } from "./client";
import type { KidsKeysSnapshot } from "./generated/KidsKeysSnapshot";
import type { ProfilesSnapshot } from "./generated/ProfilesSnapshot";
import { onProfileReset, profiles, switching } from "./profiles";
import { joined, syncNow } from "./sync";

let keys: LiveView<KidsKeysSnapshot> | null = null;
onProfileReset(async () => {
  const old = keys;
  keys = null;
  await old?.close();
});

/** Every kids profile's list as `kind:id` keys, for the owner only (null otherwise, and until the core answers).
 * Opened by its first reader and kept until the profile changes; a switch's end (done or refused) reopens it. */
export function kidsKeys(): KidsKeysSnapshot | null {
  if (switching() || !managesKids(profiles())) return null;
  keys ??= createRoot(() => liveView<KidsKeysSnapshot>("kidsKeys", {}, true));
  return keys.state.data;
}

const sync = { joined, now: syncNow };

export const allowTitle = (profile: string, kind: string, itemId: string) => writePolicy(() => op("allowTitle", { profile, kind, itemId }), sync);
export const removeTitle = (profile: string, itemId: string) => writePolicy(() => op("removeTitle", { profile, itemId }), sync);
export const makeStandard = (profile: string) => writePolicy(() => op<ProfilesSnapshot>("makeStandard", { profile }), sync);
export const createKidsProfile = (name: string, avatar: string) => writePolicy(() => op<ProfilesSnapshot>("createProfile", { name, avatar, kids: true }), sync);
