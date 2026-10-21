// The design v3 leftover gate (plans/design-v3-rollout.md, 9.3): v2 looks (glass in content,
// the accent, the gold star, edged controls, heavy weights, the v2 page colors, red destructive
// buttons) on all three apps.
//
//   bun design/gate.ts --base <ref>    a phase gate: the files changed since <ref> (committed or not)
//   bun design/gate.ts --all           every file
//   bun design/gate.ts --final         every file, the baseline ignored: phase 13 must pass this
//   bun design/gate.ts --baseline      shrink design/gate-baseline.txt to the hits still there
//   bun design/gate.ts --baseline --from <ref>   write it once, from <ref>'s tree (f923d038)
//
// A hit fails unless design/gate-allow.txt (reviewed, kept) or design/gate-baseline.txt (the v2
// leftovers the later phases remove) holds it. The baseline only shrinks: every mode fails on a
// baseline line that matches no hit (a phase deletes the lines it cleaned), and --base fails on a
// baseline line that <ref>'s tree did not have, so no phase can bless a leftover of its own. Both
// files name a hit as `<path> :: <the line, trimmed>`; an allow line may give a prefix of the line,
// or a path alone for the whole file. An allow line that matches no hit fails too, so a kept
// exception goes with the change that removes it.
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type Rule = { roots: string[]; ext: RegExp; pattern: RegExp; context?: number };

const root = join(import.meta.dir, "..");
const android = ["design", "app", "ui-tv", "ui-phone", "player"].map((m) => `apps/android/${m}/src/main`);
const rules: Rule[] = [
  {
    roots: ["apps/web/src", "apps/web/index.html"],
    ext: /\.(css|tsx?|html)$/,
    pattern:
      /var\(--glass|backdrop-filter|var\(--accent\)|var\(--star\)|inset 0 0 0 [12]px var\(--(edge|control)\)|\.eyebrow|font: ?(800|650)|font-weight: ?(800|650)|6, 9, 14|9, 13, 19|13, 18, 25|215, 232, 255|#06090E|E5372A|F5C518|background[^;]*var\(--(bad|ok|warn|star)\)/,
  },
  { roots: android, ext: /\.kt$/, pattern: /[gG]lass|colors\.accent|\.star\b|ExtraBold|FontWeight\(650\)|Eyebrow\(/ },
  // A border's color can sit on the next two lines of a multi-line `.border(` call.
  { roots: android, ext: /\.kt$/, pattern: /border\(.*(control|edge)/, context: 2 },
  {
    roots: ["apps/apple/Marquee"],
    ext: /\.swift$/,
    // `role: .destructive` draws a red button on iOS and macOS; v3 has no red (design/DECISION.md).
    pattern: /\.glass\(|Palette\.accent|Palette\.(star|ok|warn|bad)|\.heavy|Tracking\.|strokeBorder\(.*Palette\.(control|edge)|controlFill|role: \.destructive/,
  },
];

const git = (args: string[]) => {
  const r = Bun.spawnSync(["git", ...args], { cwd: root });
  if (r.exitCode !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr.toString()}`);
  return r.stdout.toString();
};

/** The files and their text, in the working tree or at a commit. */
type Tree = { files: (rule: Rule) => string[]; read: (file: string) => string };
const walk = (path: string): string[] => {
  const full = join(root, path);
  if (!existsSync(full)) return [];
  if (!statSync(full).isDirectory()) return [path];
  return readdirSync(full).flatMap((name) => walk(join(path, name)));
};
const work: Tree = { files: (r) => r.roots.flatMap(walk), read: (f) => readFileSync(join(root, f), "utf8") };
const at = (ref: string): Tree => ({
  files: (r) => git(["ls-tree", "-r", "--name-only", ref, "--", ...r.roots]).split("\n").filter(Boolean),
  read: (f) => git(["show", `${ref}:${f}`]),
});

/** Every hit in `tree` (only in `only` when given), as `<path> :: <line>` keys. */
function hits(tree: Tree, only: string[] | null = null): string[] {
  const out: string[] = [];
  for (const rule of rules) {
    for (const file of tree.files(rule).filter((f) => rule.ext.test(f) && (!only || only.includes(f)))) {
      const lines = tree.read(file).split("\n");
      lines.forEach((line, i) => {
        const text = rule.context ? lines.slice(i, i + rule.context + 1).join(" ") : line;
        if (rule.pattern.test(text) && (!rule.context || /border\(/.test(line))) out.push(`${file} :: ${line.trim()}`);
      });
    }
  }
  return out;
}

const read = (name: string) => {
  const path = join(import.meta.dir, name);
  if (!existsSync(path)) return [];
  return readFileSync(path, "utf8")
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"));
};
const allowed = read("gate-allow.txt");
const covers = (a: string, hit: string) => (a.includes(" :: ") ? hit === a || hit.startsWith(a) : hit.split(" :: ")[0] === a);
const isAllowed = (hit: string) => allowed.some((a) => covers(a, hit));
const counts = (keys: string[]) => keys.reduce((m, k) => m.set(k, (m.get(k) ?? 0) + 1), new Map<string, number>());
/** The keys of `a` that `b` does not cover, counting repeats. */
const minus = (a: string[], b: string[]) => {
  const left = counts(b);
  return a.filter((k) => {
    const n = left.get(k) ?? 0;
    if (n > 0) left.set(k, n - 1);
    return n === 0;
  });
};
const writeBaseline = (list: string[]) =>
  writeFileSync(
    join(import.meta.dir, "gate-baseline.txt"),
    `# v2 leftovers the rollout removes (design/gate.ts). It only shrinks; it is empty at phase 13 (--final).\n${list.join("\n")}\n`,
  );

const args = process.argv.slice(2);
const every = hits(work);
const now = every.filter((h) => !isAllowed(h));
const baseline = read("gate-baseline.txt");

if (args[0] === "--baseline") {
  const from = args[1] === "--from" ? args[2] : null;
  // Written once from a tree before the rollout, then only ever cut down to what is still there.
  const start = from ? hits(at(from)).filter((h) => !isAllowed(h)) : baseline;
  const kept = minus(start, minus(start, now));
  writeBaseline(kept);
  console.log(`baseline: ${kept.length} lines (${start.length - kept.length} gone)`);
  process.exit(0);
}

let files: string[] | null = null;
let ref: string | null = null;
if (args[0] === "--base" && args[1]) {
  ref = args[1];
  const changed = git(["diff", "--name-only", "--diff-filter=d", ref]);
  const untracked = git(["ls-files", "--others", "--exclude-standard"]);
  files = [...changed.split("\n"), ...untracked.split("\n")].filter(Boolean);
} else if (args[0] !== "--all" && args[0] !== "--final") {
  console.error("usage: bun design/gate.ts --base <ref> | --all | --final | --baseline [--from <ref>]");
  process.exit(2);
}

const failures: string[] = [];
const held = args[0] === "--final" ? [] : baseline;
// New leftovers: hits in the checked files that the baseline does not hold.
const checked = files ? now.filter((h) => files!.includes(h.split(" :: ")[0])) : now;
const heldHere = files ? held.filter((h) => files!.includes(h.split(" :: ")[0])) : held;
for (const h of minus(checked, heldHere)) failures.push(`new v2 leftover: ${h}`);
// A stale line: the baseline names a leftover that is gone; delete it with the change that fixed it.
for (const h of minus(held, now)) failures.push(`stale baseline line (delete it): ${h}`);
for (const a of allowed.filter((a) => !every.some((h) => covers(a, h)))) failures.push(`stale allow line (delete it): ${a}`);
// A grown baseline: a line <ref>'s tree did not have, so this change blessed a leftover of its own.
if (ref) for (const h of minus(held, hits(at(ref)).filter((x) => !isAllowed(x)))) failures.push(`baseline grew past ${ref}: ${h}`);

for (const f of failures) console.log(f);
console.log(failures.length ? `${failures.length} failures` : `no new hits, baseline ${held.length} lines`);
process.exit(failures.length ? 1 : 0);
