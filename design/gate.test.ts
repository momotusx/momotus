// The leftover gate's rules, on a throwaway repo: run with `bun test design/gate.test.ts`.
import { afterAll, expect, test } from "bun:test";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repo = mkdtempSync(join(tmpdir(), "mq-gate-"));
afterAll(() => rmSync(repo, { recursive: true, force: true }));

const sh = (...cmd: string[]) => {
  const r = Bun.spawnSync(cmd, { cwd: repo });
  return { code: r.exitCode, out: r.stdout.toString() + r.stderr.toString() };
};
const put = (path: string, text: string) => {
  mkdirSync(join(repo, path, ".."), { recursive: true });
  writeFileSync(join(repo, path), text);
};
const gate = (...args: string[]) => sh("bun", "design/gate.ts", ...args);
const commit = () => {
  sh("git", "add", "-A");
  sh("git", "-c", "user.name=gate", "-c", "user.email=gate@test", "commit", "-qm", "step");
};

mkdirSync(join(repo, "design"));
copyFileSync(join(import.meta.dir, "gate.ts"), join(repo, "design/gate.ts"));
sh("git", "init", "-q");
put("apps/web/src/a.css", ".a { color: var(--accent); }\n");
put("design/gate-allow.txt", "# none\n");
commit();
const base = sh("git", "rev-parse", "HEAD").out.trim();

test("the baseline is written from the base tree and holds its leftover", () => {
  expect(gate("--baseline", "--from", base).out).toContain("1 lines");
  expect(gate("--all").code).toBe(0);
  expect(gate("--final").code).toBe(1);
  commit();
});

test("a new leftover fails the phase gate", () => {
  put("apps/web/src/b.css", ".b { font-weight: 800; }\n");
  const r = gate("--base", base);
  expect(r.code).toBe(1);
  expect(r.out).toContain("new v2 leftover: apps/web/src/b.css");
  rmSync(join(repo, "apps/web/src/b.css"));
});

test("a fixed leftover whose baseline line stays fails as stale", () => {
  put("apps/web/src/a.css", ".a { color: var(--text); }\n");
  expect(gate("--all").out).toContain("stale baseline line");
  // --baseline cuts it, and the gate passes again.
  expect(gate("--baseline").out).toContain("0 lines (1 gone)");
  expect(gate("--base", base).code).toBe(0);
  put("apps/web/src/a.css", ".a { color: var(--accent); }\n");
  gate("--baseline", "--from", base);
});

test("a leftover blessed by hand in the baseline fails against the base", () => {
  put("apps/web/src/c.css", ".c { background: var(--glass); }\n");
  put("design/gate-baseline.txt", `# hand\napps/web/src/a.css :: .a { color: var(--accent); }\napps/web/src/c.css :: .c { background: var(--glass); }\n`);
  const r = gate("--base", base);
  expect(r.code).toBe(1);
  expect(r.out).toContain(`baseline grew past ${base}`);
  // --baseline alone never adds a line.
  gate("--baseline", "--from", base);
  expect(gate("--base", base).out).toContain("new v2 leftover: apps/web/src/c.css");
  rmSync(join(repo, "apps/web/src/c.css"));
});

test("an allow line that matches no hit fails as stale", () => {
  put("design/gate-allow.txt", "apps/web/src/a.css :: .a { color: var(--accent); }\napps/web/src/gone.css\n");
  gate("--baseline", "--from", base);
  const r = gate("--all");
  expect(r.code).toBe(1);
  expect(r.out).toContain("stale allow line (delete it): apps/web/src/gone.css");
  expect(r.out).not.toContain("a.css :: .a");
});
