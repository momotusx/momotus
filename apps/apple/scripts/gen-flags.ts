// The subtitle and audio pickers' language flags (plans/player-polish.md 2.4): HatScripts circle-flags (MIT), its
// language set, as vector images in Marquee/Resources/Flags.xcassets, and the set of tags that have one.
//
//   curl -sSfL -o cf.tgz https://registry.npmjs.org/circle-flags/-/circle-flags-2.8.3.tgz
//   mkdir cf && tar -xzf cf.tgz -C cf
//   bun apps/apple/scripts/gen-flags.ts cf/package
//
// Every flag is a 512 square under one circle mask. The mask goes (the app clips each flag to its circle), so
// the asset catalog's renderer sees flat shapes only.
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const VERSION = "2.8.3";
const pkg = process.argv[2];
if (!pkg) throw new Error("usage: bun apps/apple/scripts/gen-flags.ts <circle-flags package dir>");
const version = JSON.parse(readFileSync(join(pkg, "package.json"), "utf8")).version;
if (version !== VERSION) throw new Error(`circle-flags ${version}, expected ${VERSION}`);

const app = join(import.meta.dir, "..", "Marquee");
const out = join(app, "Resources", "Flags.xcassets");
const MASK = '<mask id="a"><circle cx="256" cy="256" r="256" fill="#fff"/></mask><g mask="url(#a)">';
// Language tags only: the question mark (xx) and the two constructed sets have no tag a track carries.
const TAG = /^(?!xx$)[a-z]{2,3}(-[a-z]{2,4})?$/;

const dir = join(pkg, "flags", "language");
const tags = readdirSync(dir)
  .filter((f) => f.endsWith(".svg"))
  .map((f) => f.slice(0, -4))
  .filter((t) => TAG.test(t))
  .sort();
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const info = { author: "xcode", version: 1 };
writeFileSync(join(out, "Contents.json"), JSON.stringify({ info }, null, 2) + "\n");
for (const tag of tags) {
  const svg = readFileSync(join(dir, `${tag}.svg`), "utf8");
  if (!svg.includes(MASK)) throw new Error(`${tag}: not one circle mask`);
  const set = join(out, `flag-${tag}.imageset`);
  mkdirSync(set);
  writeFileSync(join(set, `${tag}.svg`), svg.replace(MASK, "<g>"));
  writeFileSync(
    join(set, "Contents.json"),
    JSON.stringify(
      { images: [{ filename: `${tag}.svg`, idiom: "universal" }], info, properties: { "preserves-vector-representation": true } },
      null,
      2,
    ) + "\n",
  );
}
writeFileSync(
  join(app, "Player", "FlagSet.swift"),
  `// circle-flags ${VERSION} (MIT, HatScripts): its language set, by language tag. Written by scripts/gen-flags.ts.
enum FlagSet {
    static let tags: Set<String> = [
${tags.map((t) => `        "${t}",`).join("\n")}
    ]
}
`,
);
// MIT asks for the notice in every copy: it ships in the app beside the flags.
writeFileSync(join(app, "Resources", "LICENSE-circle-flags.txt"), readFileSync(join(pkg, "LICENSE.md"), "utf8"));
console.log(`${tags.length} flags from circle-flags ${VERSION}`);
