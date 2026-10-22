/** Accounts on the web for real (plans/accounts.md phase 2): a local relay with sign-in, the mock
 * OpenID provider (crates/mq-sync-server/examples/mock-oidc.rs, also behind the relay's Apple
 * entry), and a core per device, each page reaching its own through a routed WebSocket. Every
 * host is on this machine, so the spec runs with the network off.
 *
 * Desk signs in with the mock (a cancel, a window closed without a return, then alice): `new`,
 * Create, the recovery kit step, the doors. Its Account lists the sign-in, links Apple, unlinks it
 * after Apple's revoke, and once more without one ("Unlink anyway"). Phone signs in as bob and
 * creates an account without a kit: Home nudges for a second way in; it signs out and removes its
 * library (D13's three steps), then signs in as alice: `found`, the pairing code, Desk approves.
 * The TV's first run is the code for a phone; Desk approves it; its Home shows the Phone's join
 * until OK. A deletion asked from the link host's flow shows on Home with Cancel, and with Cancel
 * and turn off; Desk deletes the account after Apple's revoke, and the TV removes its library from
 * the account-deleted card. No ticket or receipt reaches storage, history or the URL. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes } from "node:crypto";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ACCOUNTS_PORTS, Client, PORTS, SHOTS, startBridge, startMockProvider, startRelay } from "./core";

const P = ACCOUNTS_PORTS;
const SERVER = `http://127.0.0.1:${P.relay}`;
const LINK_HOST = "link.example";
const procs: { proc: ChildProcess; data?: string }[] = [];
let scratch = "";
let desk: Page;
let phone: Page;
let tv: Page;
const errors: string[] = [];
/** The last pairing code each core's page showed, by the core's port. */
const codes = new Map<number, string>();
/** Every request a page sent its core: op and which fields it carried. */
const asked: { port: number; op?: string; view?: string; fields: string[]; form?: string }[] = [];
/** Tickets and receipts that crossed a socket. */
const secrets = new Set<string>();

function collect(port: number, text: string) {
  const walk = (v: unknown) => {
    if (!v || typeof v !== "object") return;
    const o = v as Record<string, unknown>;
    if (o.phase === "waiting" && typeof o.qr === "string") codes.set(port, o.qr);
    for (const k of ["ticket", "receipt"]) if (typeof o[k] === "string") secrets.add(o[k] as string);
    for (const x of Object.values(o)) walk(x);
  };
  walk(JSON.parse(text));
}

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
};

/** A browser context whose page talks to the core on `port`, with this relay as the one it knows. */
async function device(browser: Browser, port: number, form: keyof typeof FORMS, approver = false): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await context.addInitScript((server) => localStorage.getItem("mq.syncServer") || localStorage.setItem("mq.syncServer", server), SERVER);
  if (approver) await context.addInitScript(() => localStorage.setItem("mq.devDeviceForm", "phone"));
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = new WebSocket(`ws://127.0.0.1:${port}/ws`);
    const early: string[] = [];
    core.addEventListener("open", () => {
      for (const m of early.splice(0)) core.send(m);
    });
    core.addEventListener("message", (e) => {
      collect(port, String(e.data));
      ws.send(String(e.data));
    });
    core.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      collect(port, String(m));
      const request = JSON.parse(String(m)) as Record<string, unknown>;
      const params = request.params as Record<string, unknown> | undefined;
      asked.push({ port, op: request.op as string | undefined, view: request.view as string | undefined, fields: Object.keys(request), form: (request.form ?? params?.form) as string | undefined });
      if (core.readyState === WebSocket.OPEN) core.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => core.close());
  });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
  await page.goto(`/settings?form=${form}`);
  return page;
}

/** In-app navigation: a reload would drop the tab's sync record, as it does for a user. */
const go = async (page: Page, path: string) => {
  await page.evaluate((p) => {
    history.pushState(null, "", p);
    dispatchEvent(new PopStateEvent("popstate"));
  }, path);
};
const shot = async (page: Page, name: string) => {
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/${name}.jpg`, type: "jpeg", quality: 80 });
};
const press = async (page: Page, key: string) => {
  await page.locator(`[data-f="${key}"]`).focus();
  await page.keyboard.press("Enter");
};

/** Presses a sign-in button and answers the provider page that opens as `who` (or Cancel); the
 * page closes itself once it handed the return over. */
async function providerPage(page: Page, button: string, who: "alice" | "bob" | "Cancel") {
  const [popup] = await Promise.all([page.waitForEvent("popup"), page.locator(`[data-f="${button}"]`).click()]);
  await popup.getByText(who === "Cancel" ? "Cancel" : `Sign in as ${who}`).click();
  await popup.waitForEvent("close");
}

/** Desk approves the code another device shows (the development builds' paste page, the e2e
 * peer's path; the web itself approves nothing) and answers the six digits. */
async function approve(code: string): Promise<string> {
  await go(desk, "/sync/add");
  await desk.locator('[data-f="sync-add:code"]').fill(code);
  await desk.locator('[data-f="sync-add:approve"]').click();
  const digits = (await desk.locator(".confirm .code").textContent()) ?? "";
  expect(digits).toMatch(/^\d{6}$/);
  return digits;
}

/** Presses Sync now on Account, so the next round's report (announcements, deletion) is in. */
async function syncNow(page: Page) {
  await go(page, "/sync");
  const card = page.locator(".sync-card");
  await expect(card).toBeVisible();
  const before = Number(await card.getAttribute("data-round"));
  await page.locator('[data-f="sync:now"]').click();
  await expect.poll(async () => Number((await card.getAttribute("data-round")) ?? "Infinity"), { timeout: 30_000 }).toBeGreaterThan(before);
}

/** The link host's deletion page, as its script runs it: sign in as `who` with the brokered flow
 * returning to the page, take the ticket with the verifier, ask for the deletion. */
async function requestDeletion(provider: string, who: string): Promise<number> {
  const post = async (path: string, body: unknown) => {
    const r = await fetch(`${SERVER}${path}`, { method: "POST", headers: { "content-type": "application/json", origin: `https://${LINK_HOST}` }, body: JSON.stringify(body) });
    expect(r.headers.get("access-control-allow-origin")).toBe(`https://${LINK_HOST}`);
    expect(r.ok, `${path}: ${r.status}`).toBe(true);
    return (await r.json()) as Record<string, unknown>;
  };
  const verifier = randomBytes(32).toString("base64url");
  const started = await post(`/v1/auth/${provider}/start`, {
    verifierHash: createHash("sha256").update(verifier).digest("hex"),
    purpose: "signin",
    return: `https://${LINK_HOST}/delete`,
  });
  const picked = await fetch(`${started.url}&choose=${who}`, { redirect: "manual" });
  const back = await fetch(picked.headers.get("location") ?? "", { redirect: "manual" });
  const fragment = new URLSearchParams(new URL(back.headers.get("location") ?? "").hash.slice(1));
  expect(fragment.get("relay")).toBe("127.0.0.1");
  const taken = await post("/v1/auth/take", { flow: fragment.get("flow"), verifier, code: fragment.get("code") });
  return (await post("/v1/account/delete-request", { ticket: taken.ticket })).dueMs as number;
}

/** Everything a page keeps where other code could read it: the URL, history.state and both web storages. */
const kept = (page: Page) =>
  page.evaluate(() => {
    const out = [location.href, JSON.stringify(history.state)];
    for (const s of [localStorage, sessionStorage]) for (let i = 0; i < s.length; i++) out.push(String(s.key(i)), String(s.getItem(String(s.key(i)))));
    return out.join("\n");
  });

test.describe.serial("accounts on the web", () => {
  test.beforeAll(async ({ browser }) => {
    scratch = mkdtempSync(join(tmpdir(), "mq-accounts-e2e-"));
    const mock = await startMockProvider(P.mock);
    procs.push(mock);
    const apple = generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey.export({ type: "pkcs8", format: "pem" }).toString();
    const identity = join(scratch, "identity.json");
    writeFileSync(
      identity,
      JSON.stringify({
        key: randomBytes(32).toString("hex"),
        publicUrl: SERVER,
        linkHost: LINK_HOST,
        apple: { teamId: "TEAM", keyId: "KEY1", key: apple, servicesId: "apple-web", bundleIds: ["app.ios"], issuer: mock.url },
        oidc: [{ id: "mock", issuer: mock.url, clientId: "marquee", clientSecret: "ms" }],
      }),
    );
    procs.push(await startRelay(P.relay, undefined, ["--identity", identity, "--public-host", "127.0.0.1"]));
    for (const port of [P.a, P.b, P.c]) {
      procs.push(await startBridge(port));
      // Something to show on Home: the local sample addon (a device with none starts at the welcome steps).
      const c = new Client(`ws://127.0.0.1:${port}/ws`);
      await c.req("installAddon", { url: `http://127.0.0.1:${PORTS.source}/manifest.json` });
      c.close();
    }
    // Desk approves the others: as the e2e's peer it states a phone (only phones and tablets approve, D6).
    desk = await device(browser, P.a, "desktop", true);
    phone = await device(browser, P.b, "phone");
    tv = await device(browser, P.c, "tv");
  });

  test.afterAll(async () => {
    for (const p of [desk, phone, tv]) await p?.context().close();
    for (const { proc, data } of procs) {
      proc.kill();
      if (data) rmSync(data, { recursive: true, force: true });
    }
    if (scratch) rmSync(scratch, { recursive: true, force: true });
  });

  test("Desk signs in: a cancel says nothing, a closed window asks, then new, Create, the kit, the doors", async () => {
    await go(desk, "/welcome");
    await expect(desk.locator("h1")).toHaveText("Sign in");
    // The relay's providers, Apple first; any other by the id the relay gives it.
    await expect(desk.locator(".sign-in-providers .btn")).toHaveText(["Continue with Apple", "Continue with mock"]);
    await expect(desk.locator('[data-f="signin:other"]')).toHaveText("Other ways to sign in");
    await expect(desk.locator('[data-f="signin:not-now"]')).toHaveText("Not now");
    await shot(desk, "accounts-sign-in-desktop");

    // Cancelled on the provider page: back on the sign-in screen, nothing said.
    await providerPage(desk, "signin:mock", "Cancel");
    await expect(desk.locator('[data-f="signin:mock"]')).toBeVisible();
    await expect(desk.locator(".sign-in-failure")).toHaveCount(0);
    await expect(desk.getByText("Finish signing in in the window that opened.")).toHaveCount(0);

    // The window closed without a return, and this one is in front again: "Didn't finish signing in?"
    const [closed] = await Promise.all([desk.waitForEvent("popup"), desk.locator('[data-f="signin:mock"]').click()]);
    await expect(desk.getByText("Finish signing in in the window that opened.")).toBeVisible();
    await closed.getByText("Sign in as alice").waitFor();
    await closed.close();
    await desk.evaluate(() => dispatchEvent(new Event("focus")));
    await expect(desk.locator(".sign-in-failure .st")).toHaveText("Didn't finish signing in?");
    await expect(desk.locator(".sign-in-failure .btn")).toHaveText(["Try again", "Other ways to sign in"]);
    await shot(desk, "accounts-no-return-desktop");

    // Try again opens the provider page again, from the click.
    const [again] = await Promise.all([desk.waitForEvent("popup"), desk.locator('[data-f="signin:retry"]').click()]);
    await again.getByText("Sign in as alice").click();
    await expect(desk).toHaveURL(/\/sign-in\/new\?from=welcome$/);
    await expect(desk.locator("h1")).toHaveText("Create your Marquee account");
    await expect(desk.locator('[data-f="signin:already"]')).toHaveText("I already use Marquee on another device");
    await shot(desk, "accounts-create-desktop");
    await desk.locator('[data-f="signin:create"]').click();
    // The web has no account passkey (D11): the recovery kit step comes next, then the doors.
    await expect(desk).toHaveURL(/\/sync\/kit\?first=1&then=doors$/);
    await desk.locator('[data-f="sync-kit:make"]').click();
    await expect(desk.locator(".sync-kit-text")).toBeVisible();
    await desk.locator('[data-f="sync-kit:saved"]').click();
    await expect(desk).toHaveURL(/\/welcome\?step=doors$/);
    await expect(desk.locator('[data-f="welcome:fresh"]')).toBeVisible();
    // Signed in already: no door to another account.
    await expect(desk.locator('[data-f="welcome:pair"]')).toHaveCount(0);
    // The create carried the ticket and the web's form.
    expect(asked.find((a) => a.port === P.a && a.op === "createSyncAccount")?.fields).toEqual(expect.arrayContaining(["ticket", "form"]));
    expect(asked.find((a) => a.port === P.a && a.op === "createSyncAccount")?.form).toBe("phone");
  });

  test("Account lists the sign-in; Apple links, unlinks after its revoke, and once more without one", async () => {
    await go(desk, "/sync");
    const section = desk.locator('section[aria-label="Signed in with"]');
    const rows = section.locator(".sync-member");
    await expect(rows).toHaveCount(1);
    await expect(rows.first().locator("h3")).toHaveText("mock");
    await expect(rows.first().locator("p")).toContainText("Linked ");
    // The web signs in nothing else: it says to use the phone (4.2).
    await expect(desk.locator('[data-f="sync:add"]')).toContainText("Use your phone to sign in other devices");
    const link = async () => {
      await desk.locator('[data-f="sync:link-another"]').click();
      await expect(section.locator("[data-provider]")).toHaveText(["Continue with Apple", "Continue with mock"]);
      await providerPage(desk, "sync:link:apple", "alice");
      await expect(rows).toHaveCount(2);
      await expect(rows.nth(1).locator("h3")).toHaveText("Apple");
    };
    await link();
    await shot(desk, "accounts-signed-in-with-desktop");
    const unlinkApple = () => section.locator(".sync-member", { hasText: "Apple" }).locator('[data-f$=":unlink"]');

    // Apple's unlink: its revoke first, the receipt with the unlink.
    const [revoke] = await Promise.all([desk.waitForEvent("popup"), unlinkApple().click()]);
    await revoke.getByText("Sign in as alice").click();
    await expect(rows).toHaveCount(1);
    expect(asked.filter((a) => a.port === P.a && a.op === "unlinkIdentity").at(-1)?.fields).toContain("receipt");

    // Apple did not confirm (its window closed): the unlink goes on only on "Unlink anyway".
    await link();
    const [closed] = await Promise.all([desk.waitForEvent("popup"), unlinkApple().click()]);
    await closed.getByText("Sign in as alice").waitFor();
    await closed.close();
    await desk.evaluate(() => dispatchEvent(new Event("focus")));
    await expect(section.locator(".sign-in-failure .st")).toHaveText("Apple did not confirm, so your Apple Account may still list Marquee.");
    await expect(rows).toHaveCount(2);
    await section.locator('[data-f="sync:identity:anyway"]').click();
    await expect(rows).toHaveCount(1);
    expect(asked.filter((a) => a.port === P.a && a.op === "unlinkIdentity").at(-1)?.fields).not.toContain("receipt");
    // Apple again, for the deletion at the end.
    await link();
  });

  test("Phone signs in as bob and skips the kit: Home nudges for a second way in", async () => {
    await go(phone, "/welcome");
    await providerPage(phone, "signin:mock", "bob");
    await expect(phone).toHaveURL(/\/sign-in\/new\?from=welcome$/);
    await phone.locator('[data-f="signin:create"]').click();
    await expect(phone).toHaveURL(/\/sync\/kit\?first=1&then=doors$/);
    await phone.locator('[data-f="sync-kit:cancel"]').click();
    // The web states its form: neither it nor a TV approves another device (D6).
    expect(asked.find((a) => a.port === P.b && a.op === "createSyncAccount")?.form).toBe("web");
    await expect(phone).toHaveURL(/\/welcome\?step=doors$/);
    await go(phone, "/");
    const card = phone.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "secondWay");
    await expect(card.locator("h2")).toHaveText("Add a second way in");
    await expect(card.locator('[data-f="home:account:kit"]')).toHaveText("Make a recovery kit");
    await shot(phone, "accounts-second-way-phone");
    // Account says the same, in place of the no-kit line.
    await go(phone, "/sync");
    await expect(phone.locator(".sync-callout")).toContainText("Add a second way in");
  });

  test("Phone signs out and removes its profiles and library: leave, then Trakt and debrid, then the wipe", async () => {
    await go(phone, "/sync");
    // D13: asked on the owner profile, both ticked by default.
    await expect(phone.locator('[data-f="sync:remove-library"]')).toHaveAttribute("aria-checked", "true");
    await expect(phone.locator('[data-f="sync:remove-debrid"]')).toHaveAttribute("aria-checked", "true");
    await phone.locator('[data-f="sync:remove-library"]').click();
    await expect(phone.locator('[data-f="sync:remove-debrid"]')).toHaveCount(0);
    await phone.locator('[data-f="sync:remove-library"]').click();
    await shot(phone, "accounts-sign-out-phone");
    const from = asked.length;
    await phone.locator('[data-f="sync:leave"]').click();
    // The tab is closed the moment the leave went out: the core recorded the removal first.
    await expect.poll(() => asked.slice(from).some((a) => a.port === P.b && a.op === "signOut" && a.fields.includes("removeAfter"))).toBe(true);
    const context = phone.context();
    await phone.close();
    // The next start finds it in the profiles view and finishes steps two and three: the
    // device is as new, at the first run's sign-in.
    phone = await context.newPage();
    phone.on("pageerror", (e) => errors.push(`phone: ${e.message}`));
    await phone.goto("/");
    await expect(phone).toHaveURL(/\/welcome$/, { timeout: 30_000 });
    await expect(phone.locator("h1")).toHaveText("Sign in");
    const ops = asked.slice(from).filter((a) => a.port === P.b).map((a) => a.op);
    // In order: the leave, every profile out of Trakt, then the record-less wipe.
    const leave = ops.indexOf("signOut");
    const trakt = ops.indexOf("traktSignOut");
    const wipe = ops.lastIndexOf("signOut");
    expect(leave).toBeGreaterThanOrEqual(0);
    expect(trakt).toBeGreaterThan(leave);
    expect(wipe).toBeGreaterThan(trakt);
    // The core holds no library: its sample addon went with it.
    const c = new Client(`ws://127.0.0.1:${P.b}/ws`);
    const addons = await c.subscribe<{ fresh: boolean; addons: unknown[] }>("addons");
    expect(addons.now().addons).toEqual([]);
    c.close();
  });

  test("the TV's first run is the code for a phone: Desk approves it, and the TV skips the doors", async () => {
    await go(tv, "/welcome");
    await expect(tv.locator("h1")).toHaveText("Sign in with your phone");
    await expect(tv.getByText("On your phone, open Marquee and tap Sign in another device")).toBeVisible();
    await expect(tv.locator(".qrbox svg")).toBeVisible();
    await expect(tv.locator('[data-f="signin:tv-without"]')).toHaveText("Use without an account");
    await expect(tv.locator('[data-f="signin:tv-kit"]')).toHaveText("Use a recovery kit");
    expect(asked.find((a) => a.port === P.c && a.view === "pairing")?.form).toBe("tv");
    await shot(tv, "accounts-first-run-tv");
    const code = codes.get(P.c) ?? "";
    expect(code).toMatch(/^marquee:pair\?v=2&|^https:\/\/[^/]+\/pair#v=3&/);
    const digits = await approve(code);
    await expect(tv.locator(".confirm .code")).toHaveText(digits);
    await press(tv, "pair:yes");
    await expect(tv).toHaveURL(new RegExp(`127.0.0.1:${PORTS.vite}/$`), { timeout: 30_000 });
    await desk.locator('[data-f="sync-add:done"]').click();
  });

  test("Phone signs in as alice: found, Start over asks first, then the code Desk approves", async () => {
    await go(phone, "/welcome");
    await providerPage(phone, "signin:mock", "alice");
    await expect(phone).toHaveURL(/\/sign-in\/found\?from=welcome$/);
    await expect(phone.locator(".lead")).toHaveText(
      "This sign-in already has a Marquee account. Scan this code with your phone, where Marquee is signed in, to add this browser.",
    );
    await expect(phone.locator(".qrbox svg")).toBeVisible();
    await expect(phone.locator('[data-f="signin:found-kit"]')).toHaveText("Use a recovery kit");
    await phone.locator('[data-f="signin:cant"]').click();
    await expect(phone.locator(".sync-ask p")).toHaveText("Your old library can't be opened. It is deleted in 7 days unless one of your old devices cancels.");
    await shot(phone, "accounts-found-phone");
    await phone.locator('[data-f="signin:start-over-cancel"]').click();
    const digits = await approve(codes.get(P.b) ?? "");
    await expect(phone.locator(".confirm .code")).toHaveText(digits);
    await phone.locator('[data-f="pair:yes"]').click();
    await expect(phone).toHaveURL(new RegExp(`127.0.0.1:${PORTS.vite}/$`), { timeout: 30_000 });
    await desk.locator('[data-f="sync-add:done"]').click();
  });

  test("the TV's Home shows the Phone's join on its owner profile until OK", async () => {
    await syncNow(tv);
    await go(tv, "/");
    const card = tv.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "announcement");
    await expect(card).toContainText("A new device joined: Marquee phone.");
    // Its place: after the first row (or under the hero when there is no row), never between the hero and
    // the row that rises into it (plans/design-v3-phase2-web.md, the account card).
    const before = await card.evaluate((el) => {
      const prev = el.previousElementSibling as HTMLElement | null;
      return { prev: prev?.classList.contains("row") ? "row" : prev?.classList.contains("hero") ? "hero" : (prev?.className ?? null), rows: document.querySelectorAll(".row").length, first: el.parentElement?.querySelector(".row") === prev };
    });
    if (before.rows > 0) expect(before).toMatchObject({ prev: "row", first: true });
    else expect(before.prev).toBe("hero");
    await shot(tv, "accounts-announcement-tv");
    await press(tv, "home:account:ok");
    await expect(card).toHaveCount(0);
    expect(asked.some((a) => a.port === P.c && a.op === "ackAnnouncements")).toBe(true);
    // Gone for good: the next round does not bring it back.
    await syncNow(tv);
    await go(tv, "/");
    await expect(tv.locator(".home .hero")).toBeAttached();
    await expect(card).toHaveCount(0);
  });

  test("a deletion asked on the link host shows on Home with Cancel, and with Cancel and turn off", async () => {
    const due = await requestDeletion("mock", "alice");
    expect(due).toBeGreaterThan(Date.now() + 6 * 86_400_000);
    await syncNow(tv);
    await go(tv, "/");
    const card = tv.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "deletion");
    await expect(card).toContainText("Someone signed in with your mock account and asked to delete your Marquee account on");
    await expect(card.locator(".btn")).toHaveText(["Cancel deletion", "Cancel and turn off mock sign-in"]);
    await shot(tv, "accounts-deletion-tv");
    // Any profile cancels (D16); the card goes once the round says so.
    await press(tv, "home:account:cancel");
    await expect(card).toHaveCount(0);

    // Asked again; Desk cancels and turns the mock sign-in off, so it cannot simply ask again.
    await requestDeletion("mock", "alice");
    await syncNow(desk);
    await go(desk, "/");
    const deskCard = desk.locator(".account-card");
    await expect(deskCard).toHaveAttribute("data-card", "deletion");
    await desk.locator('[data-f="home:account:cancel-unlink"]').click();
    await expect(deskCard).toHaveCount(0);
    await go(desk, "/sync");
    await expect(desk.locator('section[aria-label="Signed in with"] .sync-member h3')).toHaveText(["Apple"]);
  });

  test("Desk deletes the account after Apple's revoke; the TV hears it and removes its library from Home", async () => {
    await go(desk, "/sync");
    await desk.locator('[data-f="sync:delete"]').click();
    const [revoke] = await Promise.all([desk.waitForEvent("popup"), desk.locator('[data-f="sync:delete-confirm"]').click()]);
    await revoke.getByText("Sign in as alice").click();
    await expect(desk.getByText("Your Marquee account was deleted. Your library stays on this device.")).toBeVisible();
    expect(asked.filter((a) => a.port === P.a && a.op === "deleteAccount").at(-1)?.fields).toContain("receipt");

    // The TV hears it at its next call to the sync service: Account's own reads, or Sync now.
    await go(tv, "/sync");
    await tv
      .locator('[data-f="sync:now"]')
      .click({ timeout: 3000 })
      .catch(() => undefined);
    await expect(tv.locator('[data-f="sync:gone-ok"]')).toBeVisible();
    await go(tv, "/");
    const card = tv.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "gone");
    await expect(card.locator(".btn")).toHaveText(["Sign in", "Remove from this device", "OK"]);
    await shot(tv, "accounts-gone-tv");
    await press(tv, "home:account:remove");
    await expect(card.locator('[data-f="home:account:remove-debrid"]')).toHaveAttribute("aria-checked", "true");
    await press(tv, "home:account:remove-confirm");
    // Removed: the TV starts as new, at its first run.
    await expect(tv).toHaveURL(/\/welcome$/, { timeout: 30_000 });
    await expect(tv.locator("h1")).toHaveText("Sign in with your phone");
  });

  test("no ticket or receipt reaches storage, history or the URL, and no page threw", async () => {
    expect(secrets.size).toBeGreaterThan(4);
    for (const page of [desk, phone, tv]) {
      const text = await kept(page);
      for (const secret of secrets) expect(text.includes(secret), `a ticket or receipt is kept on ${page.url()}`).toBe(false);
    }
    expect(errors).toEqual([]);
  });
});
