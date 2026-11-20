/** Navigation speed budget (plans/perf-nav.md): the shared walk on the TV form, every D-pad press timed from the key to
 * the end of the first frame that shows its change (the focus, the page or the field moved), at the browser's own CPU
 * and at 4x CDP throttle. The budgets are a CI runner's (about half an Apple Silicon core); the Mac gate sets
 * NAV_BUDGET_1X and NAV_BUDGET_4X to the targets (16 and 33 ms p90). A press that opens or leaves a page has its own,
 * looser budget for its slowest press (NAV_ROUTE_BUDGET_1X, _4X): a page's build varies more on a shared runner. Every
 * press must show its change: one that never does within its wait counts as over budget. NAV_BENCH=1 prints the rows
 * for bench/webos/*.jsonl. The webOS bundle on Chromium 79 and 92 is walked by apps/webos/scripts/nav-walk.ts.
 * Two structural guards ride along, as they hold on any machine: Back to Home shows the Home it left (nothing is
 * rebuilt), and a search answer builds the hits in and near its list's view, not all of them. */

import { type CDPSession, expect, type Page, test } from "./fixture";

const BUDGETS: Record<number, number> = { 1: Number(process.env.NAV_BUDGET_1X) || 50, 4: Number(process.env.NAV_BUDGET_4X) || 100 };
const ROUTE_BUDGETS: Record<number, number> = { 1: Number(process.env.NAV_ROUTE_BUDGET_1X) || 150, 4: Number(process.env.NAV_ROUTE_BUDGET_4X) || 300 };

/** `route`: the press opens or leaves a page. */
type Step = [action: string, key: string, wait?: number, route?: "route"];
const SLOW = 1500;
const WALK: Step[] = [
  ["home down", "ArrowDown"], ["home down", "ArrowDown"], ["home down", "ArrowDown"],
  ["home right", "ArrowRight"], ["home right", "ArrowRight"], ["home right", "ArrowRight"], ["home right", "ArrowRight"], ["home right", "ArrowRight"],
  // Up to Continue Watching: its title (the suite's sample film) opens with its versions, offline too.
  ["home up", "ArrowUp"], ["home up", "ArrowUp"],
  ["open title", "Enter", SLOW, "route"], ["title right", "ArrowRight"], ["play", "Enter", SLOW, "route"],
  ["back to title", "Escape", SLOW, "route"], ["back to home", "Escape", SLOW, "route"],
  ["home up", "ArrowUp"], ["home up", "ArrowUp"],
  ["bar to discover", "ArrowRight", SLOW], ["discover down", "ArrowDown"], ["discover down", "ArrowDown"],
  ["discover right", "ArrowRight"],
];

// Per key: the key's time, and the end of the first frame whose animation frame saw a new focus, path or field value
// (its rendering is done when a message posted from there arrives). A newer key ends the wait.
const PROBE = `(() => {
  window.__nav = [];
  const state = () => { const a = document.activeElement; return (a && a.getAttribute('data-f')) + '|' + location.pathname + '|' + (a && 'value' in a ? a.value : ''); };
  addEventListener('keydown', (e) => {
    const rec = { t0: Math.min(e.timeStamp, performance.now()), before: state(), change: null };
    window.__nav.push(rec);
    const loop = () => {
      if (window.__nav[window.__nav.length - 1] !== rec || performance.now() - rec.t0 > 3000) return;
      if (state() !== rec.before) {
        const ch = new MessageChannel();
        ch.port1.onmessage = () => { rec.change = performance.now() - rec.t0; };
        ch.port2.postMessage(0);
        return;
      }
      requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
  }, true);
})()`;

const focused = (page: Page) => page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.f ?? "");
const p90 = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.max(0, Math.ceil(xs.length * 0.9) - 1)] ?? NaN;

for (const rate of [1, 4]) {
  test(`tv: every press of the walk shows its change within ${BUDGETS[rate]} ms p90 at ${rate}x CPU`, async ({ browser }) => {
    test.setTimeout(180_000);
    const context = await browser.newContext({ viewport: { width: 1920, height: 1080 } });
    const page = await context.newPage();
    await page.goto("/?form=tv");
    await expect.poll(() => focused(page), { timeout: 30_000 }).toBe("hero:play");
    await page.waitForTimeout(2000);
    // Home's page before the title opens: Back must show this one again.
    await page.evaluate(() => ((document.querySelector(".home main") as unknown as { kept?: boolean }).kept = true));
    await page.evaluate(PROBE);
    const cdp: CDPSession = await context.newCDPSession(page);
    await cdp.send("Emulation.setCPUThrottlingRate", { rate });
    let keptHome: boolean | null = null;
    for (const [action, key, wait] of WALK) {
      await page.keyboard.press(key);
      await page.waitForTimeout(wait ?? 500);
      if (process.env.NAV_BENCH) console.log(`nav-budget step ${action} ${await page.evaluate(() => location.pathname)} ${await focused(page)}`);
      if (action === "back to home") keptHome = await page.evaluate(() => !!(document.querySelector(".home main") as unknown as { kept?: boolean } | null)?.kept);
    }
    await cdp.send("Emulation.setCPUThrottlingRate", { rate: 1 });
    const rows = await page.evaluate(() => (window as unknown as { __nav: { change: number | null }[] }).__nav.map((r) => r.change));
    // A press whose change never showed is over any budget.
    const walked = WALK.map(([action, , , route], i) => ({ action, route: !!route, ms: rows[i] ?? Number.POSITIVE_INFINITY }));
    if (process.env.NAV_BENCH) for (const w of walked) console.log(`nav-budget ${JSON.stringify({ date: new Date().toISOString().slice(0, 10), cpuThrottle: rate, ...w })}`);
    expect(walked.filter((w) => w.ms === Number.POSITIVE_INFINITY).map((w) => w.action), "presses that showed no change").toEqual([]);
    const presses = walked.filter((w) => !w.route).map((w) => w.ms);
    const routes = walked.filter((w) => w.route).map((w) => w.ms);
    expect(p90(presses), JSON.stringify(walked)).toBeLessThanOrEqual(BUDGETS[rate] as number);
    expect(Math.max(...routes), JSON.stringify(walked)).toBeLessThanOrEqual(ROUTE_BUDGETS[rate] as number);
    expect(keptHome, "Back from a title rebuilt Home").toBe(true);
    // A search answer builds its first screenful of hits; the rest wait until the list nears them.
    await page.goto("/search?form=tv");
    await page.locator('[data-f="search:input"]').fill("thrones");
    await expect(page.locator(".hit").first()).toBeVisible({ timeout: 30_000 });
    await page.waitForTimeout(1500);
    const results = Number((await page.locator(".palette-count").textContent({ timeout: 1000 }).catch(() => ""))?.match(/\d+/)?.[0] ?? 0);
    const hits = await page.locator(".hit").count();
    if (process.env.NAV_BENCH) console.log(`nav-budget search ${hits} of ${results} hits built`);
    expect(results, "the query needs more hits than one screenful").toBeGreaterThan(12);
    expect(hits, `${hits} of ${results} hits built at once`).toBeLessThan(results);
    await context.close();
  });
}
