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

test("an own build carries the engine on arm64-v8a only; STORE=1 builds without it", () => {
  expect(run("core_kind").out).toBe("own");
  expect(run("core_kind", { STORE: "1" }).out).toBe("store");
  expect(run("core_features arm64-v8a").out).toBe("torrent");
  expect(run("core_features armeabi-v7a").out).toBe("");
  expect(
    run("core_features arm64-v8a", { MQ_FEATURES: "torrent test-hooks" }).out,
  ).toBe("torrent test-hooks");
  // armeabi-v7a stays engine-less until its slice is shown to work, whatever MQ_FEATURES says.
  expect(run("core_features armeabi-v7a", { MQ_FEATURES: "torrent" }).out).toBe(
    "",
  );
  expect(run("core_features arm64-v8a", { STORE: "1" }).out).toBe("");
});

/** Checks a library for an ABI: the exit code and what it said. */
function check(lib: string, abi: string, env: Record<string, string> = {}) {
  return run(`core_check_engine "${lib}" ${abi} 2>&1`, env);
}

test("the check holds a library to the core its ABI and kind should be", () => {
  const dir = mkdtempSync(join(tmpdir(), "android-player-core-engine-"));
  try {
    const full = join(dir, "full.so");
    const store = join(dir, "store.so");
    // The unstripped library names the engine's crate in its symbols; a store core names the scheduler only.
    writeFileSync(full, "\u007fELF ... Cs6wwa391bSVl_9mq_engine7session ...");
    writeFileSync(
      store,
      "\u007fELF ... Cs6wwa391bSVl_15mq_engine_sched ... _10mq_runtime4core ...",
    );
    expect(check(full, "arm64-v8a").code).toBe(0);
    expect(check(store, "arm64-v8a").out).toContain("has no torrent engine");
    expect(check(store, "armeabi-v7a").code).toBe(0);
    expect(check(full, "armeabi-v7a").out).toContain(
      "stays without it until its slice is shown to work",
    );
    expect(check(store, "arm64-v8a", { STORE: "1" }).code).toBe(0);
    const extra = check(full, "arm64-v8a", { STORE: "1" });
    expect(extra.code).toBe(1);
    expect(extra.out).toContain("STORE=1 builds the store core");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// Real libraries from build-core.sh, when this machine has them: MQ_CORE_ENGINE_SO an unstripped engine core,
// MQ_CORE_STORE_SO one without the engine (each `$CARGO_TARGET_DIR/<triple>/release/libmarquee_core.so`).
const engineSo = process.env.MQ_CORE_ENGINE_SO;
const storeSo = process.env.MQ_CORE_STORE_SO;
test.skipIf(!engineSo || !storeSo)(
  "the check tells a built engine core from a built store core",
  () => {
    expect(check(engineSo as string, "arm64-v8a").code).toBe(0);
    expect(check(storeSo as string, "arm64-v8a").code).toBe(1);
    expect(check(storeSo as string, "arm64-v8a", { STORE: "1" }).code).toBe(0);
    expect(check(engineSo as string, "arm64-v8a", { STORE: "1" }).code).toBe(1);
  },
);
