/** No phone rule gives a .btn a height under 48 (QA r3: Account, sync, find addons, Calendar's failed card and the incognito
 * notice had their own 44). The e2e pin (e2e/phone-targets.spec.ts) measures the pages a shared core reaches; this reads
 * every stylesheet, so a state that needs a failure or a toggle is covered too. */

import { expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const here = import.meta.dir;

function sheets(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? sheets(path) : name.endsWith(".css") ? [path] : [];
  });
}

/** Rules whose selector names the phone form and a .btn, with a `height` or `min-height` under 48 px. */
function short(css: string): string[] {
  return [...css.matchAll(/([^{}]*)\{([^{}]*)\}/g)].flatMap(([, selector = "", body = ""]) => {
    // The button itself: a part's last compound (after the last space) holds .btn, not an svg or a ring inside it.
    const own = selector.split(",").some((part) => part.includes('[data-form="phone"]') && /\.btn\b/.test(part.trim().split(/\s+/).pop() ?? ""));
    if (!own) return [];
    const low = [...body.matchAll(/(?:^|[;\s])(?:min-)?height:\s*(\d+(?:\.\d+)?)px/g)].filter((m) => Number(m[1]) < 48);
    return low.length ? [`${selector.trim()} { ${low.map((m) => m[0].trim()).join(" ")} }`] : [];
  });
}

/** Small by design, 36 to 38 px drawn, each reaching 48 through the invisible ::after of theme/app.css (a [data-f] .btn); the
 * list is closed: a new short rule fails until it is raised or named here with its reach pinned. */
const SMALL_BY_DESIGN = [
  "screens/versions.css: [data-form=\"phone\"] .lnote .btn { height: 36px }",
  "screens/settings.css: [data-form=\"phone\"] .addon-acts .btn { height: 36px }",
  "screens/settings.css: [data-form=\"phone\"] .hrow-acts .btn { height: 36px }",
  "screens/sync.css: [data-form=\"phone\"] .sync-note .btn { height: 38px }",
  "components/download.css: [data-form=\"phone\"] .dl-sec-head .btn { height: 36px }",
];

test("phone .btn rules never set a height under 48 px", () => {
  const found = sheets(here).flatMap((file) => short(readFileSync(file, "utf8")).map((r) => `${file.slice(here.length + 1)}: ${r}`));
  expect(found.filter((r) => !SMALL_BY_DESIGN.includes(r))).toEqual([]);
});
