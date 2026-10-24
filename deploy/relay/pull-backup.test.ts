// `bun test deploy/relay`: pull-backup.sh in its dry mode, against a local folder standing in for the server's.
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const SCRIPT = join(import.meta.dir, "pull-backup.sh");
const sha = (text: string) => createHash("sha256").update(text).digest("hex");

/** A server folder holding one finished set per stamp. */
function server(stamps: string[], tamper = false): string {
  const dir = mkdtempSync(join(tmpdir(), "pull-src-"));
  for (const stamp of stamps) {
    const db = `db ${stamp}`;
    const journal = `journal ${stamp}\n`;
    writeFileSync(join(dir, `${stamp}.sqlite`), tamper ? `${db} changed` : db);
    writeFileSync(join(dir, `${stamp}.roster.jsonl`), journal);
    writeFileSync(join(dir, `${stamp}.SHA256SUMS`), `${sha(db)}  ${stamp}.sqlite\n${sha(journal)}  ${stamp}.roster.jsonl\n`);
  }
  return dir;
}

function pull(source: string, dest: string, extra: Record<string, string> = {}) {
  const notify = join(dest, "..", `notify-${Math.random()}`);
  const run = spawnSync("bash", [SCRIPT], {
    env: { PATH: process.env.PATH, HOME: dest, MQ_PULL_SOURCE: source, MQ_PULL_DEST: dest, MQ_PULL_HEALTH: "file:///dev/null", MQ_PULL_NOTIFY: notify, ...extra },
    encoding: "utf8",
  });
  return { status: run.status, log: readFileSync(join(dest, "pull.log"), "utf8"), notes: existsSync(notify) ? readFileSync(notify, "utf8") : "" };
}

const fresh = () => join(mkdtempSync(join(tmpdir(), "pull-dest-")), "backups");

test("the newest finished set is pulled and its sums checked", () => {
  const dest = fresh();
  const run = pull(server(["2026-10-24T042000Z", "2026-10-25T042000Z"]), dest);
  expect(run.status).toBe(0);
  expect(run.log).toContain("pulled: 2026-10-25T042000Z");
  expect(readdirSync(dest).filter((f) => !f.startsWith(".") && f !== "pull.log").sort()).toEqual([
    "2026-10-25T042000Z.SHA256SUMS",
    "2026-10-25T042000Z.roster.jsonl",
    "2026-10-25T042000Z.sqlite",
  ]);
  expect(run.notes).toBe("");
  expect(pull(server(["2026-10-25T042000Z"]), dest).log).toContain("up to date: 2026-10-25T042000Z");
});

test("a set that does not match its sums is not kept", () => {
  const dest = fresh();
  const run = pull(server(["2026-10-25T042000Z"], true), dest);
  expect(run.status).toBe(1);
  expect(run.log).toContain("does not match its sums");
  expect(readdirSync(dest).filter((f) => f.endsWith(".sqlite"))).toEqual([]);
});

test("seven sets stay", () => {
  const dest = fresh();
  for (let day = 10; day <= 19; day++) pull(server([`2026-10-${day}T042000Z`]), dest);
  const sets = readdirSync(dest).filter((f) => f.endsWith(".SHA256SUMS")).sort();
  expect(sets.length).toBe(7);
  expect(sets[0]).toBe("2026-10-13T042000Z.SHA256SUMS");
  expect(readdirSync(dest).filter((f) => f.endsWith(".sqlite")).length).toBe(7);
});

test("a Mac short of disk skips the pull and says so", () => {
  const dest = fresh();
  const run = pull(server(["2026-10-25T042000Z"]), dest, { MQ_PULL_MIN_FREE_KB: String(2 ** 50) });
  expect(run.log).toContain("skipped:");
  expect(readdirSync(dest).filter((f) => f.endsWith(".sqlite"))).toEqual([]);
});

test("a pull or a health check older than 48 hours shows a notification", () => {
  const dest = fresh();
  const now = Math.floor(Date.now() / 1000);
  pull(server(["2026-10-25T042000Z"]), dest, { MQ_PULL_NOW: String(now) });
  // Two days and a minute later the server has nothing new to read and the relay does not answer.
  const later = pull(mkdtempSync(join(tmpdir(), "pull-empty-")), dest, { MQ_PULL_NOW: String(now + 48 * 3600 + 60), MQ_PULL_HEALTH: "file:///nonexistent" });
  expect(later.status).toBe(1);
  expect(later.notes).toContain("No good relay pull");
  expect(later.notes).toContain("No good relay health");
});
