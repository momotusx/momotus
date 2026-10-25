// build-core.sh builds the torrent engine into every own core and fails an iOS or tvOS slice
// without it; STORE=1 builds the store core, with no engine, on purpose (plans/play-every-version.md,
// Phase 0). Run: bun test apps/apple/scripts/build-core.test.ts
import { expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const scripts = import.meta.dir;

/** A static library holding `code`, a few lines of C. */
function library(dir: string, name: string, code: string): string {
  const source = join(dir, `${name}.c`);
  writeFileSync(source, `${code}\n`);
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
    // mq-ffi's mark, as `#[unsafe(no_mangle)] pub static MQ_ENGINE_TORRENT: u8 = 1` builds it.
    engine: library(dir, "engine", "unsigned char MQ_ENGINE_TORRENT = 1;"),
    // Engine code with no mark, and a library that only uses the name: neither counts.
    store: library(dir, "store", "void _ZN9mq_engine6window17h0123456789abcdefE(void) {}\nextern unsigned char MQ_ENGINE_TORRENT;\nunsigned char read(void) { return MQ_ENGINE_TORRENT; }"),
  };
}

function check(libs: string[], env: Record<string, string> = {}) {
  const run = Bun.spawnSync([join(scripts, "check-engine.sh"), ...libs], { env: { ...process.env, ...env } });
  return { code: run.exitCode, said: run.stderr.toString(), out: run.stdout.toString() };
}

test("check-engine passes a core with the engine and fails one without", () => {
  const { dir, engine, store } = fixtures();
  try {
    expect(check([engine]).code).toBe(0);
    const without = check([engine, store]);
    expect(without.code).toBe(1);
    expect(without.said).toContain(`${store} has no torrent engine`);
    // STORE=1 is a store core on purpose: it passes and says so.
    const store1 = check([store], { STORE: "1" });
    expect(store1.code).toBe(0);
    expect(store1.out).toContain("store core");
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
  return {
    code: run.exitCode,
    said: run.stderr.toString(),
    out: run.stdout.toString(),
    cargo: readFileSync(args, "utf8").trim().split("\n"),
  };
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
    expect(run.out).toContain("store core");
    expect(run.said).toContain("stand-in: stop here");
  } finally {
    rmSync(dir, { recursive: true });
  }
});

// The libraries on disk: this checkout's linked core, when it was built, must carry the mark, as
// the Xcode build checks it.
const linked = join(scripts, "..", "Core", "MarqueeCore.xcframework");
test.if(existsSync(linked))("this checkout's linked core carries the engine", () => {
  const slices = readdirSync(linked)
    .map((slice) => join(linked, slice, "libmarquee_core.a"))
    .filter(existsSync);
  expect(slices.length).toBeGreaterThan(0);
  expect(check(slices).code).toBe(0);
});

/**
 * The Xcode pre-build phase, as project.yml writes it, run over a stand-in checkout: its
 * Core/MarqueeCore.xcframework holds `lib` as a slice, its scripts/check-engine.sh is this one.
 */
function phase(lib: string, dir: string, coreBuild = "own") {
  const yml = readFileSync(join(scripts, "..", "project.yml"), "utf8");
  const at = yml.indexOf("- name: Refuse a core without the engine");
  expect(at).toBeGreaterThan(-1);
  const lines = yml.slice(yml.indexOf("script: |", at)).split("\n").slice(1);
  const indent = lines[0].length - lines[0].trimStart().length;
  const body = lines
    .slice(0, lines.findIndex((l) => l.trim() !== "" && l.length - l.trimStart().length < indent))
    .map((l) => l.slice(indent))
    .join("\n");
  const root = join(dir, "checkout");
  mkdirSync(join(root, "Core", "MarqueeCore.xcframework", "ios-arm64"), { recursive: true });
  mkdirSync(join(root, "scripts"), { recursive: true });
  writeFileSync(join(root, "Core", "MarqueeCore.xcframework", "ios-arm64", "libmarquee_core.a"), readFileSync(lib));
  writeFileSync(join(root, "scripts", "check-engine.sh"), readFileSync(join(scripts, "check-engine.sh")));
  chmodSync(join(root, "scripts", "check-engine.sh"), 0o755);
  const run = Bun.spawnSync(["/bin/sh", "-c", body], {
    env: { ...process.env, SRCROOT: root, MARQUEE_CORE_BUILD: coreBuild },
  });
  return { code: run.exitCode, said: run.stdout.toString() + run.stderr.toString() };
}

test("the Xcode phase fails an own build on a stale core without the engine", () => {
  const { dir, engine, store } = fixtures();
  try {
    expect(phase(engine, dir).code).toBe(0);
    const stale = phase(store, dir);
    expect(stale.code).toBe(1);
    expect(stale.said).toContain("error: the linked core has no torrent engine");
    const storeBuild = phase(store, dir, "store");
    expect(storeBuild.code).toBe(0);
    expect(storeBuild.said).toContain("store core");
  } finally {
    rmSync(dir, { recursive: true });
  }
});

test("the build says own unless it is a store build, for the app to read", () => {
  const yml = readFileSync(join(scripts, "..", "project.yml"), "utf8");
  expect(yml).toContain("    MARQUEE_CORE_BUILD: own\n");
  // A Release build is a store build unless the xcodebuild line says own.
  expect(yml).toContain("  configs:\n    # A Release build (an archive) is a store build");
  expect(yml).toMatch(/    Release:\n      MARQUEE_CORE_BUILD: store\n/);
  // Every app target's Info.plist carries it.
  expect(yml.split("MarqueeCoreBuild: $(MARQUEE_CORE_BUILD)").length - 1).toBe(3);
});
