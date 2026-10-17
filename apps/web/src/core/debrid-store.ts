// Keeps the platform keystore in step with the core's debrid accounts
// (plans/core-protocol.md, Debrid). Every keystore change for a service runs
// through one serial queue, in the order the core produced the records and
// the user acted. The shared cases live in
// apps/shared/debrid-storage-vectors.json; the Android and Apple helpers
// replay the same file.

import type { DebridRecord } from "./generated/DebridRecord";
import type { DebridTombstone } from "./generated/DebridTombstone";
import type { RestoreAnswer } from "./generated/RestoreAnswer";

export interface DebridKeystore {
  records(): Promise<DebridRecord[]>;
  tombstones(): Promise<DebridTombstone[]>;
  /** One record per service: replaces whatever the service had. */
  writeRecord(record: DebridRecord): Promise<void>;
  /** Removes the service's record only when it has this generation. */
  deleteRecord(service: string, generation: string): Promise<void>;
  writeTombstone(tombstone: DebridTombstone): Promise<void>;
  deleteTombstone(tombstone: DebridTombstone): Promise<void>;
}

/** The three core calls the helper makes (`restoreDebrid`, `ackDebrid`, `disconnectDebrid`). */
export interface DebridCore {
  restore(records: DebridRecord[], tombstones: DebridTombstone[]): Promise<RestoreAnswer>;
  ack(service: string, generation: string, revision: number): Promise<void>;
  disconnect(service: string, generation: string): Promise<void>;
}

type Job = {
  generation: string;
  write: boolean;
  dropped: boolean;
  run: () => Promise<void>;
  settle: (error?: unknown) => void;
};

export class DebridStore {
  private readonly queues = new Map<string, Job[]>();
  private readonly pumps = new Map<string, Promise<void>>();
  /** Generations with a tombstone written or queued: never written again. */
  private readonly gone = new Set<string>();
  /**
   * Generations whose tombstone was stored once: never writable again, and a
   * repeated disconnect goes on from there without a new tombstone (core and
   * cleanup are idempotent), so no later failure can reopen them.
   */
  private readonly committed = new Set<string>();
  /** Tombstones whose record delete failed; retried with the next push. */
  private readonly unfinished = new Map<string, DebridTombstone>();
  private readonly disconnecting = new Map<string, Promise<void>>();

  constructor(
    private readonly keystore: DebridKeystore,
    private readonly core: DebridCore,
  ) {}

  /** At start, before any view: finish interrupted disconnects, then restore. */
  async start(): Promise<void> {
    for (const tombstone of await this.keystore.tombstones()) {
      this.gone.add(tombstone.generation);
      this.committed.add(tombstone.generation);
      await this.finish(tombstone).catch(() => this.unfinished.set(tombstone.generation, tombstone));
    }
    // Records with a tombstone go too: the core refuses them, and a tombstone
    // whose record is still stored is never handed back as deletable.
    const answer = await this.core.restore(await this.keystore.records(), await this.keystore.tombstones());
    for (const tombstone of answer.tombstones) {
      await this.keystore.deleteTombstone(tombstone).catch(() => undefined);
    }
  }

  /** Every `debrid` view push: store each pending record, then acknowledge it. */
  pending(records: DebridRecord[]): void {
    for (const tombstone of this.unfinished.values()) {
      this.unfinished.delete(tombstone.generation);
      this.enqueue(tombstone.service, tombstone.generation, false, () => this.retry(tombstone)).catch(() =>
        this.unfinished.set(tombstone.generation, tombstone),
      );
    }
    for (const record of records) {
      if (this.gone.has(record.generation)) continue;
      // A failed write is not acknowledged; the next push carries it again.
      this.enqueue(record.service, record.generation, true, () => this.save(record)).catch(() => undefined);
    }
  }

  /**
   * Disconnects one generation: tombstone, core, record, then the tombstone.
   * Rejects when the tombstone cannot be written: nothing changed, the
   * account stays connected.
   */
  disconnect(service: string, generation: string): Promise<void> {
    // One disconnect per generation at a time: a second request shares the
    // first, so a failure cannot reopen the generation under a later one.
    const running = this.disconnecting.get(generation);
    if (running) return running;
    const done = this.disconnectNow(service, generation).finally(() => this.disconnecting.delete(generation));
    this.disconnecting.set(generation, done);
    return done;
  }

  private disconnectNow(service: string, generation: string): Promise<void> {
    this.gone.add(generation);
    for (const job of this.queues.get(service) ?? []) {
      if (job.write && job.generation === generation) job.dropped = true;
    }
    const tombstone = { service, generation };
    return this.enqueue(service, generation, false, async () => {
      // An earlier disconnect stored its tombstone (its core call or cleanup may
      // have failed since): this one goes on from there, and stays gone.
      if (!this.committed.has(generation)) {
        try {
          await this.keystore.writeTombstone(tombstone);
        } catch (error) {
          this.gone.delete(generation);
          throw error;
        }
        this.committed.add(generation);
      }
      // The tombstone is durable now: from here a failure is remembered and the rest
      // (core, then record and tombstone) is tried again with the next push.
      try {
        await this.core.disconnect(service, generation);
      } catch (error) {
        this.unfinished.set(generation, tombstone);
        throw error;
      }
      await this.finish(tombstone).then(
        () => this.unfinished.delete(generation),
        () => this.unfinished.set(generation, tombstone),
      );
    });
  }

  /** Resolves once every queue is idle (tests, shutdown). */
  async idle(): Promise<void> {
    while (this.pumps.size > 0) await Promise.all(this.pumps.values());
  }

  private async save(record: DebridRecord): Promise<void> {
    const stored = (await this.keystore.records()).find((r) => r.service === record.service);
    const newer = stored?.generation === record.generation && stored.revision > record.revision;
    if (!newer) await this.keystore.writeRecord(record);
    await this.core.ack(record.service, record.generation, record.revision);
  }

  /** An interrupted disconnect: tell the core again (it is idempotent), then clean up. */
  private async retry(tombstone: DebridTombstone): Promise<void> {
    await this.core.disconnect(tombstone.service, tombstone.generation);
    await this.finish(tombstone);
  }

  /** Deletes the record, reads back that it is gone, then the tombstone. */
  private async finish(tombstone: DebridTombstone): Promise<void> {
    await this.keystore.deleteRecord(tombstone.service, tombstone.generation);
    const left = (await this.keystore.records()).some(
      (r) => r.service === tombstone.service && r.generation === tombstone.generation,
    );
    if (left) throw new Error("record still stored");
    await this.keystore.deleteTombstone(tombstone);
  }

  private enqueue(service: string, generation: string, write: boolean, run: () => Promise<void>): Promise<void> {
    return new Promise((resolve, reject) => {
      const settle = (error?: unknown) => (error === undefined ? resolve() : reject(error));
      const queue = this.queues.get(service) ?? [];
      queue.push({ generation, write, dropped: false, run, settle });
      this.queues.set(service, queue);
      if (!this.pumps.has(service)) {
        // Registered before it runs, so its own exit can remove it.
        this.pumps.set(
          service,
          Promise.resolve().then(() => this.pump(service, queue)),
        );
      }
    });
  }

  private async pump(service: string, queue: Job[]): Promise<void> {
    for (;;) {
      const job = queue.shift();
      if (!job) {
        // In the same step as the empty check: a job queued later starts
        // a new pump.
        this.pumps.delete(service);
        return;
      }
      if (job.dropped) {
        job.settle();
        continue;
      }
      try {
        await job.run();
        job.settle();
      } catch (error) {
        job.settle(error ?? new Error("failed"));
      }
    }
  }
}
