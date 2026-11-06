import { A, useNavigate } from "@solidjs/router";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Puzzle from "lucide-solid/icons/puzzle";
import Wrench from "lucide-solid/icons/wrench";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Sparkle from "lucide-solid/icons/sparkle";
import ShieldAlert from "lucide-solid/icons/shield-alert";
import { createEffect, createSignal, For, Match, on, onCleanup, Show, Switch } from "solid-js";
import { HeroCarousel } from "../components/hero";
import { CONTINUE_ROW, leftOut, RemovalNote, Row, settleRemovals } from "../components/row";
import { homeView, liveView, op, prefsView, retryHomeRow } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { HomeRow } from "../core/generated/HomeRow";
import type { DefaultAddons } from "../core/generated/DefaultAddons";
import { incognito, onProfileReset, profiles } from "../core/profiles";
import { kidsSession, kidsWords } from "../lib/kids";
import { current } from "../lib/profiles";
import { BridgeError } from "../core/bridge";
import * as sync from "../core/sync";
import { removeHere, wipeState } from "../core/wipe";
import { accountWords as aw, activeProfile, needsSecondWayIn, type Card, cancelUnlink, homeCard, urgentCard } from "../lib/account";
import { Tick } from "./sign-in";
import { cardOffer, defaultsWords } from "../lib/defaults";
import { allFailed } from "../lib/rows";
import { autoFocusSoon, inputCount } from "../focus/dom";
import { form } from "../form";
import { syncWords } from "../lib/words";
import { keptScroll } from "../lib/kept-scroll";
import "./home.css";

const pageScroll = keptScroll();
// Another profile's Home opens at the top (web journey review P3-14).
onProfileReset(pageScroll.reset);

/** Once per list version, for a profile that misses some recommended
 * addons: Review opens the step for those, Not now only closes it. Both
 * mark the version seen (synced), so it shows on no other device. */
function DefaultsCard() {
  // Incognito changes no profile's addons for good: no offer there (plans/batch-10-incognito.md 2.7).
  return <Show when={!incognito()}><OfferDefaults /></Show>;
}

function OfferDefaults() {
  const addons = liveView<AddonsSnapshot>("addons");
  const prefs = prefsView();
  const [defaults, setDefaults] = createSignal<DefaultAddons | null>(null);
  // Read again when the list changes (the store keeps its objects, so by value).
  createEffect(
    on(
      () => addons.state.data?.addons.map((a) => `${a.url} ${a.enabled}`).join("|"),
      () => void op<DefaultAddons>("defaultAddons").then(setDefaults, () => setDefaults(null)),
    ),
  );
  const kids = () => current(profiles())?.kids ?? false;
  const offer = () => cardOffer(defaults(), prefs.state.data?.defaultsSeen, kids());
  const seen = () => void op("setPrefs", { patch: { defaultsSeen: defaults()?.version } }).catch(() => undefined);
  return (
    <Show when={offer()}>
      {(n) => (
        <section class="defaults-card" aria-label={defaultsWords.cardTitle} data-fgroup="defaults-card">
          <Sparkle aria-hidden class="dc-icon" />
          <div class="dc-text">
            <h2>{defaultsWords.cardTitle}</h2>
            <p>{defaultsWords.cardLine(n())}</p>
          </div>
          <div class="acts">
            <A class="btn" href="/welcome?step=2&review=1" data-f="home:defaults:review" onClick={seen}>
              Review
            </A>
            <button class="btn" data-f="home:defaults:later" onClick={seen}>
              Not now
            </button>
          </div>
        </section>
      )}
    </Show>
  );
}

/** Home's one account card (plans/accounts.md 4.3), by priority: a scheduled deletion, the
 * account deleted elsewhere, an announcement, the nudge for a second way in. `top`: the place above the hero, for
 * the urgent ones (`urgentCard`, the owner 2026-11-02); the others sit after the first row. */
function AccountCard(props: { top?: boolean }) {
  // The account is not Incognito's (Settings hides it there too).
  return <Show when={!incognito()}><AccountNotice top={props.top} /></Show>;
}

function AccountNotice(props: { top?: boolean }) {
  const navigate = useNavigate();
  // The members name the device in a line and tell whether this one is alone. Once per Home: the top place is
  // always there, the lower one not on a Home that could not load.
  createEffect(() => {
    if (props.top && sync.joined() && !sync.devices()) void sync.loadDevices().catch(() => undefined);
  });
  // Right after a pairing, "Signed in from" the device that approved it, for 8 s: a line with no button, so
  // nothing takes focus from Home (plans/tv-qr-login.md 5.5).
  createEffect(() => {
    if (!props.top || !sync.joinedBy()) return;
    const timer = setTimeout(() => sync.setJoinedBy(null), 8000);
    onCleanup(() => clearTimeout(timer));
  });
  const any = () =>
    homeCard({
      deletion: sync.report()?.deletion ?? null,
      gone: sync.problem()?.code === "accountGone",
      announcements: sync.report()?.announcements ?? [],
      members: sync.devices()?.members ?? null,
      profile: activeProfile(profiles()),
      alone: sync.joined() && needsSecondWayIn(sync.devices()?.members ?? null),
      signedInFrom: sync.joinedBy(),
      date: (ms) => new Date(ms).toLocaleDateString(undefined, { dateStyle: "medium" }),
    });
  // This place's card: nothing at all when it is the other place's, so no room is kept for it.
  const card = () => {
    const c = any();
    return c && urgentCard(c) === !!props.top ? c : null;
  };
  const [problem, setProblem] = createSignal<string | null>(null);
  const [removing, setRemoving] = createSignal(false);
  const [removingPasskey, setRemovingPasskey] = createSignal(false);
  const [debrid, setDebrid] = createSignal(true);
  const run = (work: () => Promise<unknown>) => {
    setProblem(null);
    work().catch((e) => {
      const code = e instanceof BridgeError ? e.code : undefined;
      setProblem(code === "notOwner" ? aw.notOwner : sync.problemOf(e).words);
    });
  };
  const gone = aw.accountGoneActions;
  return (
    <Show when={card()}>
      {(c) => (
        <section class="defaults-card account-card" classList={{ urgent: !!props.top }} data-card={c().kind} aria-label={c().kind === "secondWay" ? aw.secondWayIn : c().text} data-fgroup="account-card">
          <ShieldAlert aria-hidden class="dc-icon" />
          <div class="dc-text">
            <Show when={c().kind === "secondWay"}>
              <h2>{aw.secondWayIn}</h2>
            </Show>
            <p>{"detail" in c() ? (c() as Extract<Card, { kind: "secondWay" }>).detail : c().text}</p>
            <Show when={problem()}>
              {(p) => (
                <p class="st bad" role="status">
                  <i class="dot" />
                  {p()}
                </p>
              )}
            </Show>
          </div>
          <div class="acts">
            <Switch>
              <Match when={c().kind === "deletion" && (c() as Extract<Card, { kind: "deletion" }>)}>
                {(d) => (
                  <>
                    <button class="btn primary" data-f="home:account:cancel" onClick={() => run(() => sync.cancelDeletion(false))}>
                      {aw.deletionCancel}
                    </button>
                    <Show when={d().provider}>
                      {(p) => (
                        <button class="btn" data-f="home:account:cancel-unlink" onClick={() => run(() => sync.cancelDeletion(true))}>
                          {cancelUnlink(p())}
                        </button>
                      )}
                    </Show>
                  </>
                )}
              </Match>
              <Match when={c().kind === "gone" && (c() as Extract<Card, { kind: "gone" }>)}>
                {(g) => (
                  <Show
                    when={removing()}
                    fallback={
                      <>
                        <A class="btn primary" href={form() === "tv" ? "/welcome" : "/sign-in"} data-f="home:account:sign-in">
                          {gone[0]}
                        </A>
                        <Show when={g().remove}>
                          <button class="btn" data-f="home:account:remove" onClick={() => setRemoving(true)}>
                            {gone[1]}
                          </button>
                        </Show>
                        <button class="btn" data-f="home:account:ok" onClick={sync.clearProblem}>
                          {gone[2]}
                        </button>
                      </>
                    }
                  >
                    {/* D13: the removal's own ask, the debrid accounts ticked. */}
                    <div class="acts" role="group" aria-label={aw.removeLibraryAsk}>
                      <p>{aw.removeLibraryAsk}</p>
                      <Tick on={debrid()} label={aw.removeLibraryDebrid} fkey="home:account:remove-debrid" onToggle={setDebrid} />
                      <button class="btn danger" data-f="home:account:remove-confirm" aria-busy={wipeState().running || undefined} onClick={() => run(async () => {
                        // The same passkey step as Sign out, then the removal, recorded first.
                        await sync.passkeyStep();
                        await removeHere(debrid(), true);
                      })}>
                        {gone[1]}
                      </button>
                      <button class="btn" data-f="home:account:remove-cancel" onClick={() => setRemoving(false)}>
                        {syncWords.cancel}
                      </button>
                    </div>
                  </Show>
                )}
              </Match>
              <Match when={c().kind === "announcement" && (c() as Extract<Card, { kind: "announcement" }>)}>
                {(a) => (
                  <Show
                    when={removingPasskey()}
                    fallback={
                      <>
                        {/* The owner's only (notYou needs OK), and asked once before it goes. */}
                        <Show when={a().notYou && a().member}>
                          <button class="btn" data-f="home:account:not-you" onClick={() => setRemovingPasskey(true)}>
                            {aw.announceNotYou}
                          </button>
                        </Show>
                        <Show when={a().ok}>
                          <button class="btn primary" data-f="home:account:ok" onClick={() => run(() => sync.ackAnnouncements([a().id]))}>
                            {aw.announceOk}
                          </button>
                        </Show>
                      </>
                    }
                  >
                    <div class="acts" role="group" aria-label={aw.removePasskeyAsk}>
                      <p>{aw.removePasskeyAsk}</p>
                      <button
                        class="btn danger"
                        data-f="home:account:not-you-confirm"
                        onClick={() =>
                          run(async () => {
                            await sync.removeDevice(a().member as string);
                            await sync.ackAnnouncements([a().id]);
                            setRemovingPasskey(false);
                          })
                        }
                      >
                        {syncWords.remove}
                      </button>
                      <button class="btn" data-f="home:account:not-you-cancel" onClick={() => setRemovingPasskey(false)}>
                        {syncWords.cancel}
                      </button>
                    </div>
                  </Show>
                )}
              </Match>
              <Match when={c().kind === "secondWay"}>
                <button class="btn primary" data-f="home:account:kit" onClick={() => navigate("/sync/kit")}>
                  {syncWords.kitTitle}
                </button>
              </Match>
            </Switch>
          </div>
        </section>
      )}
    </Show>
  );
}

export default function Home() {
  const home = homeView();
  // A profile that never had an addon list starts at the welcome steps.
  const navigate = useNavigate();
  const addons = liveView<AddonsSnapshot>("addons");
  createEffect(() => {
    if (addons.state.data?.fresh && !kidsSession(profiles())) navigate("/welcome", { replace: true });
  });
  const [page, setPage] = createSignal<HTMLElement | null>(null);
  const hasContinue = () => home.rows[0]?.rowId === CONTINUE_ROW;
  // A title removed from Continue Watching is left out until its Undo passes and the core's row drops it.
  createEffect(() => home.status.state === "open" && settleRemovals(home.rows.find((r) => r.rowId === CONTINUE_ROW)?.tiles.map((t) => t.id) ?? []));
  const shown = (row: HomeRow) => (row.rowId === CONTINUE_ROW ? { ...row, tiles: row.tiles.filter((t) => !leftOut(t.id)) } : row);
  const empty = () => home.status.state === "open" && home.rows.length === 0;
  const hero = () => home.heroes.length > 0;
  // Rows still on their way: only then is a missing hero still coming. When
  // every row has answered and none gave a backdrop (offline, posters only),
  // the hero closes instead of shimmering for good.
  const waiting = () => home.status.state === "opening" || home.rows.some((r) => r.state.state === "loading" || r.state.state === "slow");
  // The first focus is the hero's Play, as tvOS and Android TV (plans/design-v3-phase2-web.md, rule 1):
  // the top shelf fills the first screen, so a focused card would sit half off it and the page would
  // scroll the hero away. With no hero there or coming, nothing is marked and the focus goes to the first
  // content, Continue Watching (focus/dom.ts, the first-content fallback). A hero that lands after the
  // focus went to the row anyway (restore's late fallback) takes it back, with the page at its top,
  // unless the viewer has pressed a key since Home opened.
  // Read when Home mounts. The Home view stays alive across Back, so a Back-restored tile is not pulled to
  // Play by a late hero; if Home ever re-subscribes on Back, skip the return after a restore.
  const opened = inputCount();
  createEffect(
    on(
      hero,
      (has, had) => {
        if (!has || had || inputCount() !== opened) return;
        if (!(document.activeElement instanceof HTMLElement) || !document.activeElement.closest("[data-row]")) return;
        autoFocusSoon(() => document.querySelector<HTMLElement>('[data-f="hero:play"], [data-f="hero:details"]'));
      },
      { defer: true },
    ),
  );

  const attach = (el: HTMLElement) => {
    setPage(el);
    pageScroll.attach(el);
  };

  return (
    <div class="home">
      <main class="page" data-vscroll ref={attach}>
        {/* A deletion, the account gone, a new device: first, before the hero, never kept for later. */}
        <AccountCard top />
        <HeroCarousel heroes={home.heroes} waiting={waiting()} />

        {/* With no row to follow, the account's notices sit under the hero. */}
        <Show when={home.rows.length === 0}>
          <AccountCard />
        </Show>

        <Show when={home.status.state === "failed" && home.status}>
          {(s) => (
            <section class="problem home-problem">
              <CircleAlert aria-hidden class="problem-icon" />
              <h2>Momotus cannot reach its core</h2>
              <p>{"reason" in s() ? (s() as { reason: string }).reason : ""}</p>
              <button class="btn primary" data-f="home:retry" data-autofocus onClick={() => location.reload()}>
                <RefreshCw aria-hidden />
                Try again
              </button>
            </section>
          )}
        </Show>

        {/* A kids profile's Home is its list: a grown-up fills it (plans/m11-kids.md 6.4). */}
        <Show when={empty() && kidsSession(profiles())}>
          <section class="problem home-problem">
            <p>{kidsWords.empty}</p>
          </section>
        </Show>
        <Show when={empty() && !kidsSession(profiles())}>
          <section class="problem home-problem">
            <h2>Home fills up from your addons</h2>
            <p>{defaultsWords.emptyHome}</p>
            <div class="acts">
              {/* Past first run: the recommended addons to switch, not the first-run doors. */}
              <A class="btn primary" href={addons.state.data?.fresh ? "/welcome" : "/welcome?step=2&review=1"} data-f="home:setup" data-autofocus>
                <Wrench aria-hidden />
                Set up Momotus
              </A>
              <A class="btn" href="/addons" data-f="home:addons">
                <Puzzle aria-hidden />
                Add an addon
              </A>
            </div>
          </section>
        </Show>

        <Show
          when={!hero() && allFailed(home.rows)}
          fallback={
            <>
              {/* Under Continue Watching, or first when there is none; an empty
                  Home already offers Set up Momotus, so not beside it. */}
              <Show when={!hasContinue() && !empty()}>
                <DefaultsCard />
              </Show>
              <For each={home.rows}>
                {(row, i) => (
                  <>
                    {/* A Continue Watching whose every title was just removed leaves with them. */}
                    <Show when={row.rowId !== CONTINUE_ROW || row.tiles.length === 0 || shown(row).tiles.length > 0}>
                      {/* Home opens at its top, where the hero leaves room for a row or two: the rows past those start far. */}
                      <Row row={shown(row)} root={page} onRetry={retryHomeRow} startNear={i() < 3} />
                    </Show>
                    <Show when={i() === 0 && row.rowId === CONTINUE_ROW}>
                      <RemovalNote />
                    </Show>
                    {/* The account's notices and the defaults card come after the first row, never over the
                        hero's art or between it and the row that rises into it (as Android's TV Home). */}
                    <Show when={i() === 0}>
                      <AccountCard />
                    </Show>
                    <Show when={i() === 0 && row.rowId === CONTINUE_ROW}>
                      <DefaultsCard />
                    </Show>
                  </>
                )}
              </For>
            </>
          }
        >
          <section class="problem home-problem">
            <CircleAlert aria-hidden class="problem-icon" />
            <h2>Home could not load</h2>
            <p>None of your addons answered. Check the network, then try again.</p>
            <button class="btn primary" data-f="home:retry-lanes" data-autofocus onClick={() => location.reload()}>
              <RefreshCw aria-hidden />
              Try again
            </button>
          </section>
        </Show>
      </main>
    </div>
  );
}
