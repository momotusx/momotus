import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const script = join(import.meta.dir, "core-engine.sh");

/** Runs `body` in bash with core-engine.sh sourced and only `env` set: its output and exit code. */
function run(
  body: string,
  env: Record<string, string> = {},
): { out: string; code: number } {
  const done = Bun.spawnSync(["bash", "-c", `source "${script}"; ${body}`], {
    env: { PATH: process.env.PATH ?? "", ...env },
  });
  return { out: done.stdout.toString().trim(), code: done.exitCode };
}

test("an own build carries the torrent engine; STORE=1 builds without it, MQ_FEATURES names them outright", () => {
  expect(run("core_features").out).toBe("torrent");
  expect(run("core_features", { STORE: "1" }).out).toBe("");
  expect(run("core_features", { MQ_FEATURES: "" }).out).toBe("");
  expect(run("core_features", { MQ_FEATURES: "torrent test-hooks" }).out).toBe(
    "torrent test-hooks",
  );
  expect(run("core_features", { STORE: "1", MQ_FEATURES: "torrent" }).out).toBe(
    "torrent",
  );
});

test("a library without the engine fails an own build, one with it fails a store build", () => {
  const dir = mkdtempSync(join(tmpdir(), "android-player-core-engine-"));
  try {
    const full = join(dir, "full.so");
    const store = join(dir, "store.so");
    // The unstripped library names the engine's crate in its symbols; a store core names the scheduler only.
    writeFileSync(full, "\u007fELF ... Cs6wwa391bSVl_9mq_engine7session ...");
    writeFileSync(store, "\u007fELF ... Cs6wwa391bSVl_15mq_engine_sched ... _10mq_runtime4core ...");
    expect(run(`core_check_engine "${full}"`).code).toBe(0);
    const missing = run(`core_check_engine "${store}" 2>&1`);
    expect(missing.code).toBe(1);
    expect(missing.out).toContain("has no torrent engine");
    expect(run(`core_check_engine "${store}"`, { STORE: "1" }).code).toBe(0);
    const extra = run(`core_check_engine "${full}" 2>&1`, { STORE: "1" });
    expect(extra.code).toBe(1);
    expect(extra.out).toContain("STORE=1 builds the store core");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
