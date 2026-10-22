// build-core.sh builds the torrent engine into every own core and fails an iOS or tvOS slice
// without it; STORE=1 builds the store core, with no engine, on purpose (plans/play-every-version.md,
// Phase 0). Run: bun test apps/apple/scripts/build-core.test.ts
import { expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const scripts = import.meta.dir;

/** A static library holding one function under `symbol`, as a Rust crate's mangled name. */
function library(dir: string, name: string, symbol: string): string {
  const source = join(dir, `${name}.c`);
  writeFileSync(source, `void ${symbol}(void) {}\n`);
  const object = join(dir, `${name}.o`);
  const lib = join(dir, `${name}.a`);
  for (const args of [["clang", "-c", source, "-o", object], ["ar", "rcs", lib, object]]) {
    expect(Bun.spawnSync(args).exitCode).toBe(0);
  }
  return lib;
}

function fixtures() {
  const dir = mkdtempSync(join(tmpdir(), "mq-engine-check-"));
  return {
    dir,
    engine: library(dir, "engine", "_ZN9mq_engine6window17h0123456789abcdefE"),
    // The scheduler crate is in every core; its name starts like the engine's.
    store: library(dir, "store", "_ZN15mq_engine_sched4plan17h0123456789abcdefE"),
  };
}

function check(...libs: string[]) {
  const run = Bun.spawnSync([join(scripts, "check-engine.sh"), ...libs]);
  return { code: run.exitCode, said: run.stderr.toString() };
}

test("check-engine passes a core with the engine and fails one without", () => {
  const { dir, engine, store } = fixtures();
  try {
    expect(check(engine).code).toBe(0);
    const without = check(engine, store);
    expect(without.code).toBe(1);
    expect(without.said).toContain(`${store} has no torrent engine`);
  } finally {
    rmSync(dir, { recursive: true });
  }
});

/**
 * Runs build-core.sh with stand-ins: `cargo` records its arguments and writes `lib` as every slice;
 * `uniffi-bindgen` stops the script before it touches apps/apple/Core.
 */
function build(lib: string, dir: string, env: Record<string, string> = {}) {
  const bin = join(dir, "bin");
  mkdirSync(bin, { recursive: true });
  const args = join(dir, "cargo-args");
  const target = join(dir, "target");
  writeFileSync(
    join(bin, "cargo"),
    `#!/bin/bash\necho "$*" >> "${args}"\nwhile [[ $# -gt 0 ]]; do [[ $1 == --target ]] && t=$2; shift; done\nmkdir -p "${target}/$t/release" && cp "${lib}" "${target}/$t/release/libmarquee_core.a"\n`,
  );
  writeFileSync(join(bin, "uniffi-bindgen"), "#!/bin/bash\necho 'stand-in: stop here' >&2\nexit 3\n");
  for (const tool of ["cargo", "uniffi-bindgen"]) chmodSync(join(bin, tool), 0o755);
  const run = Bun.spawnSync([join(scripts, "build-core.sh")], {
    env: { ...process.env, ...env, PATH: `${bin}:${process.env.PATH}`, CARGO_TARGET_DIR: target },
  });
  return { code: run.exitCode, said: run.stderr.toString(), cargo: readFileSync(args, "utf8").trim().split("\n") };
}

test("an own build asks for the engine and fails a slice without it", () => {
  const { dir, store } = fixtures();
  try {
    const run = build(store, dir);
    expect(run.cargo.length).toBeGreaterThan(0);
    for (const line of run.cargo) expect(line).toContain("--features torrent");
    expect(run.code).toBe(1);
    expect(run.said).toContain("has no torrent engine");
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("an own build with the engine goes on past the check", () => {
  const { dir, engine } = fixtures();
  try {
    const run = build(engine, dir);
    expect(run.said).not.toContain("has no torrent engine");
    expect(run.said).toContain("stand-in: stop here");
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("STORE=1 builds the store core with no engine, on purpose", () => {
  const { dir, store } = fixtures();
  try {
    const run = build(store, dir, { STORE: "1" });
    for (const line of run.cargo) expect(line).not.toContain("--features");
    expect(run.said).not.toContain("has no torrent engine");
    expect(run.said).toContain("stand-in: stop here");
  } finally {
    rmSync(dir, { recursive: true });
  }
});
