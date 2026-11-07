/** The device's debrid accounts (plans/c10-handoff.md, phase 3a). A browser has
 * no keystore that can hold a key safely, so records live in this tab's memory;
 * the core keeps the accounts while it runs. One `debrid` subscription for the
 * app's life hands every push's pending records to the helper, after the
 * restore it starts with. */

import { createSignal } from "solid-js";
import { bridge, op } from "./client";
import { DebridStore, type DebridKeystore } from "./debrid-store";
import { WebosKeystore } from "./webos-debrid";
import { isWebOS } from "../lib/webos";
import type { DebridRecord } from "./generated/DebridRecord";
import type { DebridSnapshot } from "./generated/DebridSnapshot";
import type { DebridTombstone } from "./generated/DebridTombstone";
import type { RestoreAnswer } from "./generated/RestoreAnswer";

class TabKeystore implements DebridKeystore {
  private stored = new Map<string, DebridRecord>();
  private stones: DebridTombstone[] = [];
  async records() {
    return [...this.stored.values()];
  }
  async tombstones() {
    return [...this.stones];
  }
  async writeRecord(record: DebridRecord) {
    this.stored.set(record.service, record);
  }
  async deleteRecord(service: string, generation: string) {
    if (this.stored.get(service)?.generation === generation) this.stored.delete(service);
  }
  async writeTombstone(tombstone: DebridTombstone) {
    if (!this.stones.some((t) => t.generation === tombstone.generation)) this.stones.push(tombstone);
  }
  async deleteTombstone(tombstone: DebridTombstone) {
    this.stones = this.stones.filter((t) => t.generation !== tombstone.generation);
  }
}

// A TV keeps them in its service's keystore (src/core/webos-debrid.ts).
export const debridStore = new DebridStore(isWebOS() ? new WebosKeystore() : new TabKeystore(), {
  restore: (records, tombstones) => op<RestoreAnswer>("restoreDebrid", { records, tombstones }),
  ack: (service, generation, revision) => op("ackDebrid", { service, generation, revision }).then(() => undefined),
  disconnect: (service, generation) => op("disconnectDebrid", { service, generation }).then(() => undefined),
});

const [accounts, setAccounts] = createSignal<DebridSnapshot | null>(null);
/** The connected services and the records not stored yet; null until the core answered. */
export { accounts as debridAccounts };

let ready = false;
const got = (snapshot: DebridSnapshot) => {
  setAccounts(snapshot);
  if (ready) debridStore.pending(snapshot.pending);
};
void debridStore
  .start()
  .catch((e) => console.warn("debrid restore failed", e instanceof Error ? e.message : e))
  .finally(() => {
    ready = true;
    const now = accounts();
    if (now) debridStore.pending(now.pending);
  });
void bridge
  .subscribe<DebridSnapshot, DebridSnapshot>("debrid", {}, {
    snapshot: got,
    updates: (updates) => {
      const last = updates[updates.length - 1];
      if (last) got(last);
    },
  })
  .catch(() => undefined);
