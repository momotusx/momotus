import { expect, test } from "bun:test";
import type { DebridRecord } from "./generated/DebridRecord";
import { WebosKeystore, type Wire } from "./webos-debrid";

const rec = (service: string, generation: string) => ({ service, generation, revision: 1, credential: { kind: "key", key: "k" } }) as unknown as DebridRecord;

class SlowWire implements Wire {
  all: Record<string, unknown> = {};
  failNext = false;
  async read() {
    return structuredClone(this.all);
  }
  async write(key: string, value: unknown) {
    await Bun.sleep(1);
    if (this.failNext) {
      this.failNext = false;
      throw new Error("disk full");
    }
    this.all = { ...this.all, [key]: structuredClone(value) };
  }
}

test("two services written at once both stay", async () => {
  const wire = new SlowWire();
  const keystore = new WebosKeystore(wire);
  await Promise.all([keystore.writeRecord(rec("torbox", "g1")), keystore.writeRecord(rec("realdebrid", "g2"))]);
  const again = new WebosKeystore(wire);
  expect((await again.records()).map((r) => r.service).sort()).toEqual(["realdebrid", "torbox"]);
});

test("a write the service did not store changes nothing, here or after a restart", async () => {
  const wire = new SlowWire();
  const keystore = new WebosKeystore(wire);
  await keystore.writeRecord(rec("torbox", "g1"));
  wire.failNext = true;
  await expect(keystore.writeRecord(rec("realdebrid", "g2"))).rejects.toThrow("disk full");
  expect((await keystore.records()).map((r) => r.service)).toEqual(["torbox"]);
  expect((await new WebosKeystore(wire).records()).map((r) => r.service)).toEqual(["torbox"]);
  // The next write still goes.
  await keystore.writeRecord(rec("premiumize", "g3"));
  expect((await new WebosKeystore(wire).records()).map((r) => r.service)).toEqual(["torbox", "premiumize"]);
});
