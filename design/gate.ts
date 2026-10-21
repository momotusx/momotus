// The design v3 leftover gate (plans/design-v3-rollout.md, 9.3): v2 looks (glass in content,
// the accent, the gold star, edged controls, heavy weights, the v2 page colors) on all three apps.
//
//   bun design/gate.ts --base <ref>   the files changed since <ref> (committed or not)
//   bun design/gate.ts --all          every file, as at phase 13
//   bun design/gate.ts --baseline     write design/gate-baseline.txt from today's hits
//
// A hit fails unless design/gate-allow.txt (reviewed, kept) or design/gate-baseline.txt (the v2
// leftovers the later phases remove, empty at phase 13) holds it. Both name a hit as
// `<path> :: <the line, trimmed>`; an allow line may give a prefix of the line, or a path alone
// for the whole file.
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type Rule = { roots: string[]; ext: RegExp; pattern: RegExp; context?: number };

const root = join(import.meta.dir, "..");
const rules: Rule[] = [
  {
    roots: ["apps/web/src", "apps/web/index.html"],
    ext: /\.(css|tsx?|html)$/,
    pattern:
      /var\(--glass|backdrop-filter|var\(--accent\)|var\(--star\)|inset 0 0 0 [12]px var\(--(edge|control)\)|\.eyebrow|font: ?(800|650)|font-weight: ?(800|650)|6, 9, 14|9, 13, 19|13, 18, 25|215, 232, 255|#06090E|E5372A|F5C518|background[^;]*var\(--(bad|ok|warn|star)\)/,
  },
  {
    roots: ["design", "app", "ui-tv", "ui-phone", "player"].map((m) => `apps/android/${m}/src/main`),
    ext: /\.kt$/,
    pattern: /[gG]lass|colors\.accent|\.star\b|ExtraBold|FontWeight\(650\)|Eyebrow\(/,
  },
  // A border's color can sit on the next two lines of a multi-line `.border(` call.
  {
    roots: ["design", "app", "ui-tv", "ui-phone", "player"].map((m) => `apps/android/${m}/src/main`),
    ext: /\.kt$/,
    pattern: /border\(.*(control|edge)/,
    context: 2,
  },
  {
    roots: ["apps/apple/Marquee"],
    ext: /\.swift$/,
    pattern: /\.glass\(|Palette\.accent|Palette\.(star|ok|warn|bad)|\.heavy|Tracking\.|strokeBorder\(.*Palette\.(control|edge)|controlFill/,
  },
];

const walk = (path: string): string[] => {
  const full = join(root, path);
  if (!existsSync(full)) return [];
  if (!statSync(full).isDirectory()) return [path];
  return readdirSync(full).flatMap((name) => walk(join(path, name)));
};

/** Every hit in [files], as `<path> :: <line>` keys. */
function hits(files: string[] | null): string[] {
  const out: string[] = [];
  for (const rule of rules) {
    const all = rule.roots.flatMap(walk).filter((f) => rule.ext.test(f));
    for (const file of files ? all.filter((f) => files.includes(f)) : all) {
      const lines = readFileSync(join(root, file), "utf8").split("\n");
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

const counts = (keys: string[]) => keys.reduce((m, k) => m.set(k, (m.get(k) ?? 0) + 1), new Map<string, number>());
const allowed = read("gate-allow.txt");
const isAllowed = (hit: string) => {
  const [path] = hit.split(" :: ");
  return allowed.some((a) => (a.includes(" :: ") ? hit === a || hit.startsWith(a) : path === a));
};

const args = process.argv.slice(2);
if (args[0] === "--baseline") {
  const list = hits(null).filter((h) => !isAllowed(h));
  writeFileSync(join(import.meta.dir, "gate-baseline.txt"), `# v2 leftovers the rollout removes (design/gate.ts); empty at phase 13.\n${list.join("\n")}\n`);
  console.log(`wrote ${list.length} baseline hits`);
  process.exit(0);
}

let files: string[] | null = null;
if (args[0] === "--base" && args[1]) {
  const diff = Bun.spawnSync(["git", "diff", "--name-only", "--diff-filter=d", args[1]], { cwd: root });
  const untracked = Bun.spawnSync(["git", "ls-files", "--others", "--exclude-standard"], { cwd: root });
  files = [...diff.stdout.toString().split("\n"), ...untracked.stdout.toString().split("\n")].filter(Boolean);
} else if (args[0] !== "--all") {
  console.error("usage: bun design/gate.ts --base <ref> | --all | --baseline");
  process.exit(2);
}

const left = counts(read("gate-baseline.txt"));
const failed: string[] = [];
for (const hit of hits(files)) {
  if (isAllowed(hit)) continue;
  const n = left.get(hit) ?? 0;
  if (n > 0) left.set(hit, n - 1);
  else failed.push(hit);
}
for (const f of failed) console.log(`new v2 leftover: ${f}`);
console.log(failed.length ? `${failed.length} new hits` : "no new hits");
process.exit(failed.length ? 1 : 0);
