// The language flags on every platform from one rule (plans/onboarding-language-picker.md, Flags):
// circle-flags' language set, with design/flag-overrides.json applied, written to the web set and
// handed to the Apple and Android generators, so the three cannot drift.
//
//   curl -sSfL -o cf.tgz https://registry.npmjs.org/circle-flags/-/circle-flags-2.8.3.tgz
//   mkdir cf && tar -xzf cf.tgz -C cf
//   bun design/flags.ts cf/package
//
// The downloaded package is never changed: the overlay (the package's language set plus each
// override under the language's own tag, minus the glyph languages' files) is built in a temp folder.
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const pkg = process.argv[2];
if (!pkg) throw new Error("usage: bun design/flags.ts <circle-flags package dir>");
const root = join(import.meta.dir, "..");
type Overrides = { country: Record<string, { set: "country" | "language"; file: string }>; glyph: string[] };
const overrides: Overrides = JSON.parse(readFileSync(join(import.meta.dir, "flag-overrides.json"), "utf8"));
/** Language tags only: the question mark (xx) and the constructed sets have no tag a track carries. */
export const TAG = /^(?!xx$)[a-z]{2,3}(-[a-z0-9]{2,4})?$/;

// The generators check it too, but only after the web set is written.
const VERSION = "2.8.3";
const version = JSON.parse(readFileSync(join(pkg, "package.json"), "utf8")).version;
if (version !== VERSION) throw new Error(`circle-flags ${version}, expected ${VERSION}`);

const overlay = mkdtempSync(join(tmpdir(), "circle-flags-overlay-"));
try {
  const from = join(pkg, "flags", "language");
  const to = join(overlay, "flags", "language");
  mkdirSync(to, { recursive: true });
  for (const f of ["package.json", "LICENSE.md"]) copyFileSync(join(pkg, f), join(overlay, f));
  for (const f of readdirSync(from).filter((f) => f.endsWith(".svg"))) copyFileSync(join(from, f), join(to, f));
  for (const [tag, { set, file }] of Object.entries(overrides.country)) {
    const source = set === "country" ? join(pkg, "flags", `${file}.svg`) : join(from, `${file}.svg`);
    if (!existsSync(source)) throw new Error(`${tag}: no ${set} file ${file}`);
    copyFileSync(source, join(to, `${tag.toLowerCase()}.svg`));
  }
  for (const tag of overrides.glyph) rmSync(join(to, `${tag.toLowerCase()}.svg`), { force: true });
  const glyphs = overrides.glyph.map((t) => t.toLowerCase()).sort();
  writeFileSync(join(overlay, "glyph.json"), JSON.stringify(glyphs));

  // The web set: every tag's file, and the glyph languages by name.
  const web = join(root, "apps", "web", "src", "player", "flags");
  for (const f of readdirSync(web).filter((f) => f.endsWith(".svg"))) rmSync(join(web, f));
  const tags = readdirSync(to).filter((f) => f.endsWith(".svg") && TAG.test(f.slice(0, -4)));
  for (const f of tags) copyFileSync(join(to, f), join(web, f));
  copyFileSync(join(pkg, "LICENSE.md"), join(web, "LICENSE.md"));
  writeFileSync(
    join(root, "apps", "web", "src", "lib", "flag-glyphs.ts"),
    `// The languages drawn with the neutral language glyph, not a flag. Written by design/flags.ts.\nexport const GLYPHS: ReadonlySet<string> = new Set(${JSON.stringify(glyphs)});\n`,
  );
  console.log(`web: ${tags.length} flags, ${glyphs.length} glyph languages`);

  for (const script of [join(root, "apps", "apple", "scripts", "gen-flags.ts"), join(root, "apps", "android", "scripts", "flags.ts")]) {
    const run = Bun.spawnSync(["bun", script, overlay], { stdout: "inherit", stderr: "inherit" });
    if (run.exitCode !== 0) throw new Error(`${script} failed`);
  }
} finally {
  rmSync(overlay, { recursive: true, force: true });
}
