import { expect, test } from "bun:test";
import vectors from "../../../shared/debrid-storage-vectors.json";
import { type DebridCore, type DebridKeystore, DebridStore } from "./debrid-store";
import type { DebridRecord } from "./generated/DebridRecord";
import type { DebridTombstone } from "./generated/DebridTombstone";

type Call = "writeRecord" | "deleteRecord" | "writeTombstone" | "deleteTombstone";

class FakeKeystore implements DebridKeystore {
  readonly stored = new Map<string, DebridRecord>();
  stones: DebridTombstone[] = [];
  readonly log: string[] = [];
  readonly failing = new Map<Call, number>();
  holding: Call | null = null;
  release: () => void = () => undefined;

  private async gate(call: Call): Promise<void> {
    const left = this.failing.get(call) ?? 0;
    if (left > 0) {
      this.failing.set(call, left - 1);
      this.log.push(`${call} failed`);
      throw new Error(call);
    }
    if (this.holding === call) {
      this.holding = null;
      await new Promise<void>((resolve) => {
        this.release = resolve;
      });
    }
  }

  async records() {
    return [...this.stored.values()];
  }
  async tombstones() {
    return [...this.stones];
  }
  async writeRecord(record: DebridRecord) {
    await this.gate("writeRecord");
    this.stored.set(record.service, record);
    this.log.push(`writeRecord ${record.generation} ${record.revision}`);
  }
  async deleteRecord(service: string, generation: string) {
    await this.gate("deleteRecord");
    if (this.stored.get(service)?.generation === generation) this.stored.delete(service);
    this.log.push(`deleteRecord ${generation}`);
  }
  async writeTombstone(tombstone: DebridTombstone) {
    await this.gate("writeTombstone");
    this.stones.push(tombstone);
    this.log.push(`writeTombstone ${tombstone.generation}`);
  }
  async deleteTombstone(tombstone: DebridTombstone) {
    await this.gate("deleteTombstone");
    this.stones = this.stones.filter((t) => t.generation !== tombstone.generation);
    this.log.push(`deleteTombstone ${tombstone.generation}`);
  }
}

/** Answers `restore` the way the core does. */
class FakeCore implements DebridCore {
  readonly acks: string[] = [];
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
    this.disconnects.push(`${service} ${generation}`);
  }
}

const id = (r: DebridRecord) => `${r.service} ${r.generation} ${r.revision}`;
const stone = (t: DebridTombstone) => `${t.service} ${t.generation}`;

type Step = Record<string, unknown> & { op: string };

for (const vector of vectors.cases) {
  test(vector.name, async () => {
    const keystore = new FakeKeystore();
    for (const r of vector.keystore.records as DebridRecord[]) keystore.stored.set(r.service, r);
    keystore.stones = [...(vector.keystore.tombstones as DebridTombstone[])];
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
          keystore.failing.set(step.call as Call, step.times as number);
          break;
        case "settle":
          await store.idle();
          await Bun.sleep(0);
          break;
        case "restart":
          store = new DebridStore(keystore, core);
          await store.start();
          break;
        case "expect": {
          const want = step as Record<string, unknown>;
          if ("log" in want) expect(keystore.log).toEqual(want.log as string[]);
          if ("records" in want) expect([...keystore.stored.values()].map(id)).toEqual(want.records as string[]);
          if ("tombstones" in want) expect(keystore.stones.map(stone)).toEqual(want.tombstones as string[]);
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
