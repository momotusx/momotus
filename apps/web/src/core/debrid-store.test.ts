import { expect, test } from "bun:test";
import vectors from "../../../shared/debrid-storage-vectors.json";
import { type DebridCore, type DebridKeystore, DebridStore } from "./debrid-store";
import type { DebridRecord } from "./generated/DebridRecord";
import type { DebridTombstone } from "./generated/DebridTombstone";
import { WebosKeystore, type Wire } from "./webos-debrid";

type Call = "writeRecord" | "deleteRecord" | "writeTombstone" | "deleteTombstone";

/** The tab's keystore (src/core/debrid.ts): records in memory. */
class MemoryKeystore implements DebridKeystore {
  readonly stored = new Map<string, DebridRecord>();
  stones: DebridTombstone[] = [];
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
    this.stones.push(tombstone);
  }
  async deleteTombstone(tombstone: DebridTombstone) {
    this.stones = this.stones.filter((t) => t.generation !== tombstone.generation);
  }
}

/** The webOS service's keystore: one JSON entry that outlives the page (a `restart` reads it again). */
class FakeWire implements Wire {
  all: Record<string, unknown> = {};
  async read() {
    return JSON.parse(JSON.stringify(this.all)) as Record<string, unknown>;
  }
  async write(key: string, value: unknown) {
    this.all = { ...this.all, [key]: JSON.parse(JSON.stringify(value)) };
  }
}

/** Holds and fails calls by kind, logs the ones that finished, and passes them to the keystore under test. */
class FakeKeystore implements DebridKeystore {
  constructor(public inner: DebridKeystore) {}
  readonly log: string[] = [];
  readonly failing = new Map<Call, number>();
  holding: Call | null = null;
  release: () => void = () => undefined;

  private async gate(call: Call): Promise<void> {
    if (this.holding === call) {
      this.holding = null;
      await new Promise<void>((resolve) => {
        this.release = resolve;
      });
    }
    const left = this.failing.get(call) ?? 0;
    if (left > 0) {
      this.failing.set(call, left - 1);
      this.log.push(`${call} failed`);
      throw new Error(call);
    }
  }

  records() {
    return this.inner.records();
  }
  tombstones() {
    return this.inner.tombstones();
  }
  async writeRecord(record: DebridRecord) {
    await this.gate("writeRecord");
    await this.inner.writeRecord(record);
    this.log.push(`writeRecord ${record.generation} ${record.revision}`);
  }
  async deleteRecord(service: string, generation: string) {
    await this.gate("deleteRecord");
    await this.inner.deleteRecord(service, generation);
    this.log.push(`deleteRecord ${generation}`);
  }
  async writeTombstone(tombstone: DebridTombstone) {
    await this.gate("writeTombstone");
    await this.inner.writeTombstone(tombstone);
    this.log.push(`writeTombstone ${tombstone.generation}`);
  }
  async deleteTombstone(tombstone: DebridTombstone) {
    await this.gate("deleteTombstone");
    await this.inner.deleteTombstone(tombstone);
    this.log.push(`deleteTombstone ${tombstone.generation}`);
  }
}

/** Answers `restore` the way the core does. */
class FakeCore implements DebridCore {
  readonly acks: string[] = [];
  failing = 0;
  readonly disconnects: string[] = [];
  readonly restores: { records: string[]; tombstones: string[] }[] = [];

  async restore(records: DebridRecord[], tombstones: DebridTombstone[]) {
    this.restores.push({ records: records.map(id), tombstones: tombstones.map(stone) });
    const gone = new Set(tombstones.map((t) => t.generation));
    const handed = new Set(records.map((r) => r.generation));
    return {
      refused: records.filter((r) => gone.has(r.generation)).map((r) => r.generation),
      tombstones: tombstones.filter((t) => !handed.has(t.generation)),
    };
  }
  async ack(service: string, generation: string, revision: number) {
    this.acks.push(`${service} ${generation} ${revision}`);
  }
  async disconnect(service: string, generation: string) {
    if (this.failing > 0) {
      this.failing--;
      throw new Error("disconnect");
    }
    this.disconnects.push(`${service} ${generation}`);
  }
}

const id = (r: DebridRecord) => `${r.service} ${r.generation} ${r.revision}`;
const stone = (t: DebridTombstone) => `${t.service} ${t.generation}`;

type Step = Record<string, unknown> & { op: string };

// Each case runs over the tab's keystore and over the webOS one, whose `restart` reads the service's entry again.
const kinds = {
  tab: () => {
    const inner = new MemoryKeystore();
    return { inner, again: () => inner as DebridKeystore };
  },
  webos: () => {
    const wire = new FakeWire();
    return { inner: new WebosKeystore(wire) as DebridKeystore, again: () => new WebosKeystore(wire) };
  },
};

for (const [kind, make] of Object.entries(kinds))
for (const vector of vectors.cases) {
  test(`${kind}: ${vector.name}`, async () => {
    const made = make();
    for (const r of vector.keystore.records as DebridRecord[]) await made.inner.writeRecord(r);
    for (const t of vector.keystore.tombstones as DebridTombstone[]) await made.inner.writeTombstone(t);
    const keystore = new FakeKeystore(made.inner);
    const core = new FakeCore();
    const errors: string[] = [];
    let store = new DebridStore(keystore, core);
    for (const step of vector.steps as Step[]) {
      switch (step.op) {
        case "pending":
          store.pending(step.records as DebridRecord[]);
          break;
        case "disconnect": {
          const name = `${step.service} ${step.generation}`;
          store.disconnect(step.service as string, step.generation as string).catch(() => errors.push(name));
          break;
        }
        case "hold":
          keystore.holding = step.call as Call;
          break;
        case "release":
          // Let the held call reach its gate first.
          await Bun.sleep(5);
          keystore.release();
          break;
        case "fail":
          if (step.call === "disconnect") core.failing = step.times as number;
          else keystore.failing.set(step.call as Call, step.times as number);
          break;
        case "settle":
          await store.idle();
          await Bun.sleep(0);
          break;
        case "restart":
          keystore.inner = made.again();
          store = new DebridStore(keystore, core);
          await store.start();
          break;
        case "expect": {
          const want = step as Record<string, unknown>;
          if ("log" in want) expect(keystore.log).toEqual(want.log as string[]);
          if ("records" in want) expect((await keystore.records()).map(id)).toEqual(want.records as string[]);
          if ("tombstones" in want) expect((await keystore.tombstones()).map(stone)).toEqual(want.tombstones as string[]);
          if ("acks" in want) expect(core.acks).toEqual(want.acks as string[]);
          if ("disconnects" in want) expect(core.disconnects).toEqual(want.disconnects as string[]);
          if ("restores" in want) expect(core.restores).toEqual(want.restores as never);
          if ("errors" in want) expect(errors).toEqual(want.errors as string[]);
          break;
        }
        default:
          throw new Error(`unknown op ${step.op}`);
      }
      // Time passes between steps: a queued job starts.
      await Bun.sleep(1);
    }
  });
}
