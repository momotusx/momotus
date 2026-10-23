import { expect, test } from "bun:test";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
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
  // The ABIs whose core carries the engine, for the app (BuildConfig.CORE_ENGINE_ABIS).
  expect(run("core_engine_abis arm64-v8a armeabi-v7a").out).toBe("arm64-v8a");
  expect(run("core_engine_abis armeabi-v7a").out).toBe("");
  expect(
    run("core_engine_abis arm64-v8a armeabi-v7a", { STORE: "1" }).out,
  ).toBe("");
});

// The NDK's tools: llvm-nm reads the libraries, its clang makes the stand-in ones.
const ndk =
  process.env.ANDROID_NDK_HOME ??
  "/opt/homebrew/share/android-commandlinetools/ndk/27.1.12297006";
const bin = join(ndk, "toolchains/llvm/prebuilt/darwin-x86_64/bin");
const nm = join(bin, "llvm-nm");
const clang = join(bin, "aarch64-linux-android24-clang");
const hasNdk = existsSync(nm) && existsSync(clang);

/** Checks a library for an ABI: the exit code and what it said. */
function check(lib: string, abi: string, env: Record<string, string> = {}) {
  return run(`core_check_engine "${lib}" ${abi} 2>&1`, { NM: nm, ...env });
}

/** A stand-in core library built from `c`: an arm64 ELF shared object, as cargo-ndk makes. */
function library(dir: string, name: string, c: string): string {
  const source = join(dir, `${name}.c`);
  const out = join(dir, `${name}.so`);
  writeFileSync(source, c);
  const made = Bun.spawnSync([clang, "-shared", "-fPIC", "-o", out, source]);
  expect(made.exitCode).toBe(0);
  return out;
}

test.skipIf(!hasNdk)(
  "the check holds a library to the core its ABI and kind should be",
  () => {
    const dir = mkdtempSync(join(tmpdir(), "android-player-core-engine-"));
    try {
      // As mq-ffi: the torrent feature exports MQ_ENGINE_TORRENT; a store core does not define it.
      const full = library(
        dir,
        "full",
        '__attribute__((visibility("default"), used)) const unsigned char MQ_ENGINE_TORRENT = 1;\n',
      );
      const store = library(
        dir,
        "store",
        '__attribute__((visibility("default"))) int mq_core_open(void) { return 0; }\n',
      );
      // Naming the mark is not carrying it: a library that only refers to it has it undefined.
      const refers = library(
        dir,
        "refers",
        "extern const unsigned char MQ_ENGINE_TORRENT;\nconst unsigned char *mark(void) { return &MQ_ENGINE_TORRENT; }\n",
      );
      expect(check(full, "arm64-v8a").code).toBe(0);
      expect(check(store, "arm64-v8a").out).toContain("has no torrent engine");
      expect(check(refers, "arm64-v8a").out).toContain("has no torrent engine");
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
  },
);

// Real libraries from build-core.sh, when this machine has them: MQ_CORE_ENGINE_SO an engine core, MQ_CORE_STORE_SO
// one without it (`$CARGO_TARGET_DIR/<triple>/release/libmarquee_core.so`, or the stripped one in jniLibs).
const engineSo = process.env.MQ_CORE_ENGINE_SO;
const storeSo = process.env.MQ_CORE_STORE_SO;
test.skipIf(!hasNdk || !engineSo || !storeSo)(
  "the check tells a built engine core from a built store core",
  () => {
    expect(check(engineSo as string, "arm64-v8a").code).toBe(0);
    expect(check(storeSo as string, "arm64-v8a").code).toBe(1);
    expect(check(storeSo as string, "arm64-v8a", { STORE: "1" }).code).toBe(0);
    expect(check(engineSo as string, "arm64-v8a", { STORE: "1" }).code).toBe(1);
  },
);
