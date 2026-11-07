/** The debrid records on a webOS TV (plans/webos.md section 4, Keystore): one `debrid` entry in the app service's
 * keystore. The page is its only writer, so the copy read at start stays current; each change writes the whole entry,
 * one at a time, and becomes the copy only once the service says it is on disk. */

import type { DebridKeystore } from "./debrid-store";
import type { DebridRecord } from "./generated/DebridRecord";
import type { DebridTombstone } from "./generated/DebridTombstone";
import { keystore } from "./webos-transport";

type Entry = { records: DebridRecord[]; tombstones: DebridTombstone[] };
export type Wire = { read(): Promise<Record<string, unknown>>; write(key: string, value: unknown): Promise<void> };

const ENTRY = "debrid";
const serviceWire: Wire = {
  read: () => keystore({ op: "read" }),
  write: (key, value) => keystore({ op: "write", key, value }).then(() => undefined),
};

export class WebosKeystore implements DebridKeystore {
  private held: Promise<Entry> | null = null;
  private turn: Promise<unknown> = Promise.resolve();
  constructor(private readonly wire: Wire = serviceWire) {}

  private load(): Promise<Entry> {
    this.held ??= this.wire.read().then(
      (all) => (all[ENTRY] as Entry | undefined) ?? { records: [], tombstones: [] },
      (e) => {
        this.held = null;
        throw e;
      },
    );
    return this.held;
  }

  private change(edit: (entry: Entry) => Entry): Promise<void> {
    const run = this.turn.then(async () => {
      const next = edit(await this.load());
      await this.wire.write(ENTRY, next);
      this.held = Promise.resolve(next);
    });
    this.turn = run.catch(() => undefined);
    return run;
  }

  async records() {
    return [...(await this.load()).records];
  }
  async tombstones() {
    return [...(await this.load()).tombstones];
  }
  writeRecord(record: DebridRecord) {
    return this.change((e) => ({ ...e, records: [...e.records.filter((r) => r.service !== record.service), record] }));
  }
  deleteRecord(service: string, generation: string) {
    return this.change((e) => ({ ...e, records: e.records.filter((r) => r.service !== service || r.generation !== generation) }));
  }
  writeTombstone(tombstone: DebridTombstone) {
    return this.change((e) => (e.tombstones.some((t) => t.generation === tombstone.generation) ? e : { ...e, tombstones: [...e.tombstones, tombstone] }));
  }
  deleteTombstone(tombstone: DebridTombstone) {
    return this.change((e) => ({ ...e, tombstones: e.tombstones.filter((t) => t.generation !== tombstone.generation) }));
  }
}
