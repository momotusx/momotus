/** Sign-in on the web (plans/accounts.md 4.1, 4.2, 5): the first screen with the providers the
 * sync service offers, the other ways, a sync server of one's own, Create your Marquee account
 * after `new`, the pairing code after `found` with Start over, and the TV's "Sign in with your
 * phone". Built from the welcome steps' own pieces; every word comes from
 * `apps/shared/account-words.json` (lib/account.ts). */

import { A, Navigate, useNavigate, useSearchParams } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import LifeBuoy from "lucide-solid/icons/life-buoy";
import QrCode from "lucide-solid/icons/qr-code";
import Server from "lucide-solid/icons/server";
import { createResource, createSignal, For, type JSX, onCleanup, onMount, Show } from "solid-js";
import { profiles } from "../core/profiles";
import { held, openSignInWindow, relayProviders, runSignIn, type SignInEnd, type SignInRun, setHeld, signInServer } from "../core/sign-in";
import * as sync from "../core/sync";
import { autoFocusSoon } from "../focus/dom";
import { form } from "../form";
import { resetHistoryBase } from "../history";
import { type Act, accountWords as w, continueWith, type Failure, providerOrder, signInFailure } from "../lib/account";
import { mayMakeKit } from "../lib/sync";
import { plainAddress, syncWords } from "../lib/words";
import { Session } from "./pairing";
import "./browse.css";
import Brand from "../components/brand";
import "./settings.css";
import "./onboarding.css";

type From = "welcome" | "account";
const fromOf = (q: { from?: string | string[] }): From => (q.from === "welcome" ? "welcome" : "account");
const tail = (from: From) => (from === "welcome" ? "?from=welcome" : "");
/** Where the first screen is: the welcome step, or its own page from Account. */
const firstScreen = (from: From) => (from === "welcome" ? "/welcome" : "/sign-in");
/** After creating: the recovery kit step (the web has no account passkey, D11), then the doors
 * on a first run. A TV, or a profile that may not make kits (D17), goes straight on. */
const afterCreate = (from: From) =>
  form() === "tv" || !mayMakeKit(profiles()) ? (from === "welcome" ? "/welcome?step=doors" : "/sync") : `/sync/kit?first=1${from === "welcome" ? "&then=doors" : ""}`;
const codeOf = (e: unknown) => {
  const c = e && typeof e === "object" && "code" in e ? (e as { code?: unknown }).code : undefined;
  return typeof c === "string" ? c : "";
};

/** A failure's line and its buttons, the first the primary. */
export function FailureNote(props: { failure: Failure; fkey: string; onAct: (act: Act) => void }) {
  return (
    <div class="sign-in-failure">
      <p class="st bad" role="status">
        <i class="dot" />
        {props.failure.text}
      </p>
      <Show when={props.failure.actions.length}>
        <div class="acts">
          <For each={props.failure.actions}>
            {(a, i) => (
              <button class="btn" classList={{ primary: i() === 0 }} data-f={`${props.fkey}:${a.act}`} onClick={() => props.onAct(a.act)}>
                {a.label}
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

/** A checkbox in the chips' own look (Sign out's D13 ask). */
export function Tick(props: { on: boolean; label: string; fkey: string; fixed?: boolean; onToggle: (on: boolean) => void }) {
  return (
    <button
      class="chip"
      classList={{ on: props.on }}
      role="checkbox"
      aria-checked={props.on}
      aria-disabled={props.fixed || undefined}
      data-f={props.fkey}
      onClick={() => !props.fixed && props.onToggle(!props.on)}
    >
      <Show when={props.on}>
        <Check aria-hidden class="chip-check" />
      </Show>
      {props.label}
    </button>
  );
}

/** One provider sign-in at a time, started from a click (the window opens in it). `fail` words
 * a failure (null: nothing to say); a newer start drops what an older one brings. */
export function useSignIn(server: () => string, done: (end: Exclude<SignInEnd, { phase: "failed" }>, provider: string) => void, fail = signInFailure) {
  const [run, setRun] = createSignal<SignInRun | null>(null);
  const [failure, setFailure] = createSignal<Failure | null>(null);
  const [provider, setProvider] = createSignal("");
  const go = (p: string, revoke = false) => {
    run()?.cancel();
    const win = openSignInWindow();
    const r = runSignIn(win, server(), p, revoke);
    setFailure(null);
    setProvider(p);
    setRun(r);
    void r.done.then((end) => {
      if (run() !== r) return;
      setRun(null);
      if (end.phase === "failed") setFailure(fail(end.code, p));
      else done(end, p);
    });
  };
  onCleanup(() => run()?.cancel());
  /** What shows under the buttons: the window is open, the user came back without a return, or a failure. */
  const said = (): { waiting: boolean; failure: Failure | null } => {
    const r = run();
    if (r?.stalled()) return { waiting: false, failure: fail("noRedirect", provider()) };
    return { waiting: !!r, failure: r ? null : failure() };
  };
  return { go, said, provider, setFailure, stop: () => run()?.cancel(), again: (revoke = false) => go(provider(), revoke) };
}

/** The window's state under a sign-in's buttons. */
export function SignInState(props: { said: { waiting: boolean; failure: Failure | null }; fkey: string; onAct: (act: Act) => void }) {
  return (
    <>
      <Show when={props.said.waiting}>
        <p class="st warn" role="status">
          <i class="dot" />
          {w.signInWindow}
        </p>
      </Show>
      <Show when={props.said.failure}>{(f) => <FailureNote failure={f()} fkey={props.fkey} onAct={props.onAct} />}</Show>
    </>
  );
}

/** The welcome steps' frame, for the sign-in pages outside them. */
function Frame(props: { label: string; children: JSX.Element }) {
  onMount(() => autoFocusSoon(() => document.querySelector<HTMLElement>(".welcome [data-autofocus]")));
  return (
    <main class="welcome page" data-vscroll>
      <header class="welcome-head">
        <div class="brand">
          <Brand />
        </div>
      </header>
      <section class="step-body" aria-label={props.label}>
        {props.children}
      </section>
    </main>
  );
}

/** Home, the root, once this device joined; after the first pull, so Home opens with the account's addons. */
async function homeAfterPull(navigate: (to: string, o?: { replace?: boolean }) => void) {
  await sync.syncNow();
  resetHistoryBase();
  navigate("/", { replace: true });
}

/** The first screen (4.1.1): Continue with each provider the sync service offers, Other ways to
 * sign in, Not now. With no sync service known, or one that offers none, `doors` instead. */
export function SignInStart(props: { from: From; doors: () => JSX.Element }) {
  const navigate = useNavigate();
  const server = signInServer();
  const [providers, { refetch }] = createResource(() => server || false, relayProviders);
  const s = useSignIn(
    () => server,
    (end, provider) => {
      if (end.phase === "receipt") return;
      setHeld({ server, provider, ticket: end.ticket, account: end.phase === "found" ? end.account : null });
      navigate(`/sign-in/${end.phase}${tail(props.from)}`);
    },
  );
  const leave = () => navigate(props.from === "welcome" ? "/welcome?step=doors" : "/sync");
  const act = (a: Act) => {
    if (a === "retry") s.again();
    else if (a === "other") navigate(`/sign-in/other${tail(props.from)}`);
    else if (a === "notNow") leave();
    else s.setFailure(null);
  };
  const doors = () => !server || (providers.state === "ready" && providers().length === 0);
  return (
    <Show when={!doors()} fallback={props.doors()}>
      <section class="step-body" aria-label={w.signInTitle} aria-busy={providers.loading || undefined}>
        <h1>{w.signInTitle}</h1>
        <Show when={props.from === "account"}>
          <p class="lead">{w.libraryAdded}</p>
        </Show>
        <Show
          when={providers.state !== "errored"}
          fallback={<FailureNote failure={signInFailure("unreachable") as Failure} fkey="signin:relay" onAct={(a) => (a === "retry" ? void refetch() : leave())} />}
        >
          <div class="acts sign-in-providers" data-fgroup="providers">
            <For each={providers.state === "ready" ? providerOrder(providers()) : []}>
              {(p, i) => (
                <button class="btn" classList={{ primary: i() === 0 }} data-f={`signin:${p}`} data-provider={p} data-autofocus={i() === 0 ? "" : undefined} onClick={() => s.go(p)}>
                  {continueWith(p)}
                </button>
              )}
            </For>
          </div>
          <SignInState said={s.said()} fkey="signin" onAct={act} />
          <div class="acts">
            <A class="btn" href={`/sign-in/other${tail(props.from)}`} data-f="signin:other">
              {w.otherWays}
            </A>
            <button class="btn" data-f="signin:not-now" onClick={leave}>
              {w.notNow}
            </button>
          </div>
        </Show>
      </section>
    </Show>
  );
}

/** `/sign-in`, from Account on a device that used Not now. */
export default function SignInScreen() {
  const [q] = useSearchParams();
  return (
    <Show when={!sync.joined()} fallback={<Navigate href="/sync" />}>
      <main class="welcome page" data-vscroll>
        <header class="welcome-head">
          <div class="brand">
            <Brand />
          </div>
        </header>
        <SignInStart from={fromOf(q)} doors={() => <Navigate href="/sync" />} />
      </main>
    </Show>
  );
}

/** Other ways to sign in: a recovery kit, a signed-in device (the pairing code), a sync server of one's own. */
export function OtherWays() {
  const [q] = useSearchParams();
  const door = (href: string, fkey: string, icon: JSX.Element, label: string, first = false) => (
    <A class="door" href={href} data-f={fkey} data-autofocus={first ? "" : undefined}>
      {icon}
      <span>
        <b>{label}</b>
      </span>
      <ChevronRight aria-hidden class="chev" />
    </A>
  );
  return (
    <Frame label={w.otherWays}>
      <h1>{w.otherWays}</h1>
      <div class="doors" data-fgroup="other-ways">
        {door("/sync/recover", "signin:kit", <LifeBuoy aria-hidden />, w.useRecoveryKit, true)}
        {door("/pair", "signin:scan", <QrCode aria-hidden />, w.scanFromDevice)}
        {door(`/sign-in/server${tail(fromOf(q))}`, "signin:server", <Server aria-hidden />, w.useOwnServer)}
      </div>
    </Frame>
  );
}

/** Use my own sync server: one that offers sign-in goes back to the first screen with its
 * providers; one that offers none to today's doors on Account. */
export function OwnServer() {
  const [q] = useSearchParams();
  const navigate = useNavigate();
  const [server, setServer] = createSignal(sync.rememberedServer());
  const [failure, setFailure] = createSignal<Failure | null>(null);
  const [busy, setBusy] = createSignal(false);
  const use = async () => {
    const s = server().trim();
    // An address with a user name or password is never kept or sent.
    if (!s || !plainAddress(s)) return setFailure({ text: syncWords.badAddress, actions: [] });
    setBusy(true);
    setFailure(null);
    try {
      const offered = await relayProviders(s);
      sync.rememberJoin(s, sync.defaultDeviceName());
      navigate(offered.length ? firstScreen(fromOf(q)) : "/sync", { replace: true });
    } catch {
      setFailure(signInFailure("unreachable"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Frame label={w.useOwnServer}>
      <h1>{w.useOwnServer}</h1>
      <form
        class="install"
        onSubmit={(e) => {
          e.preventDefault();
          void use();
        }}
      >
        <label class="field-line">
          <span class="kicker">{w.ownServerLabel}</span>
          <input
            type="url"
            placeholder={syncWords.serverHint}
            value={server()}
            onInput={(e) => setServer(e.currentTarget.value)}
            data-f="signin:server-field"
            data-autofocus
            autocomplete="off"
            autocapitalize="off"
            spellcheck={false}
          />
        </label>
        <Show when={failure()}>{(f) => <FailureNote failure={f()} fkey="signin:server" onAct={(a) => (a === "retry" ? void use() : navigate(firstScreen(fromOf(q))))} />}</Show>
        <button class="btn primary" type="submit" data-f="signin:server-use" aria-busy={busy() || undefined}>
          {w.ownServerUse}
        </button>
      </form>
    </Frame>
  );
}

/** The work a sign-in's ticket was held for, run again with a fresh ticket after "Sign in again to finish." */
type Pending = (ticket: string) => Promise<void>;

/** The ticket's own failures and the window of a second sign-in, for `new` and `found`. */
function useHeldTicket(from: From) {
  const navigate = useNavigate();
  const [busy, setBusy] = createSignal(false);
  const [failure, setFailure] = createSignal<Failure | null>(null);
  let pending: Pending | null = null;
  /** The code of the last failed op: a stale ticket signs in again, anything else runs the op again. */
  let failed = "";
  const run = async (work: Pending) => {
    const h = held();
    if (!h || busy()) return;
    pending = work;
    setBusy(true);
    setFailure(null);
    try {
      await work(h.ticket);
    } catch (e) {
      failed = codeOf(e);
      // The sign-in got an account meanwhile: that is `found`, whose Start over warns first.
      if (failed === "linkedElsewhere" && !h.account) return navigate(`/sign-in/found${tail(from)}`, { replace: true });
      setFailure(signInFailure(failed, h.provider));
    } finally {
      setBusy(false);
    }
  };
  // "Sign in again to finish": the same provider, then what was asked, with the new ticket.
  const again = useSignIn(
    () => held()?.server ?? "",
    (end, provider) => {
      const h = held();
      if (!h || end.phase === "receipt") return;
      setHeld({ ...h, provider, ticket: end.ticket, account: end.phase === "found" ? end.account : null });
      if (end.phase === "found" && !h.account) return navigate(`/sign-in/found${tail(from)}`, { replace: true });
      if (pending) void run(pending);
    },
  );
  const act = (a: Act) => {
    if (a === "retry") {
      if (!failure() || failed === "ticketExpired") again.again();
      else if (pending) void run(pending);
    } else if (a === "other") navigate(`/sign-in/other${tail(from)}`);
    else {
      setFailure(null);
      again.setFailure(null);
    }
  };
  const said = () => {
    const s = again.said();
    return { waiting: s.waiting, failure: s.failure ?? failure() };
  };
  return { run, busy, said, act };
}

/** `new`: Create your Marquee account, or "I already use Marquee on another device": the pairing
 * code, then the sign-in is linked with the ticket held. */
export function NewAccount() {
  const [q] = useSearchParams();
  const from = fromOf(q);
  const navigate = useNavigate();
  const name = sync.defaultDeviceName();
  const t = useHeldTicket(from);
  const [pairing, setPairing] = createSignal(0);
  const create = (move = false) =>
    t.run(async (ticket) => {
      const h = held();
      if (!h) return;
      await sync.createAccount(h.server, name, "", ticket, move);
      sync.rememberJoin(h.server, name);
      setHeld(null);
      navigate(afterCreate(from), { replace: true });
    });
  const link = () =>
    t.run(async (ticket) => {
      await sync.linkIdentity(ticket);
      setHeld(null);
      await homeAfterPull(navigate);
    });
  return (
    <Show when={held()} fallback={<Navigate href={firstScreen(from)} />}>
      {(h) => (
        <Frame label={w.createAccount}>
          <h1>{w.createAccount}</h1>
          <Show when={from === "account"}>
            <p class="lead">{w.libraryAdded}</p>
          </Show>
          <Show
            when={pairing()}
            keyed
            fallback={
              <div class="acts">
                <button class="btn primary" data-f="signin:create" data-autofocus aria-busy={t.busy() || undefined} onClick={() => void create()}>
                  {w.createAccount}
                </button>
                <button class="btn" data-f="signin:already" onClick={() => setPairing(1)}>
                  {w.alreadyUse}
                </button>
              </div>
            }
          >
            {(n) => <Session server={h().server} name={name} how={w.tvSignInHow} onJoined={() => void link()} onDone={() => setPairing(n + 1)} />}
          </Show>
          <SignInState said={t.said()} fkey="signin:new" onAct={(a) => t.act(a)} />
        </Frame>
      )}
    </Show>
  );
}

/** `found`: this sign-in has an account; the web joins it by phone (the pairing code), else a
 * recovery kit, else Start over, which moves the sign-in to a new account and deletes the old
 * one in 7 days unless one of its devices cancels. */
export function Found() {
  const [q] = useSearchParams();
  const from = fromOf(q);
  const navigate = useNavigate();
  const name = sync.defaultDeviceName();
  const t = useHeldTicket(from);
  const [run, setRun] = createSignal(1);
  const [asking, setAsking] = createSignal(false);
  const startOver = () =>
    t.run(async (ticket) => {
      const h = held();
      if (!h) return;
      await sync.createAccount(h.server, name, "", ticket, true);
      sync.rememberJoin(h.server, name);
      setHeld(null);
      navigate(afterCreate(from), { replace: true });
    });
  const joined = () => {
    setHeld(null);
    void homeAfterPull(navigate);
  };
  return (
    <Show when={held()} fallback={<Navigate href={firstScreen(from)} />}>
      {(h) => (
        <Frame label={w.signInTitle}>
          <h1>{w.signInTitle}</h1>
          <p class="lead">{w.foundWeb}</p>
          <Show when={run()} keyed>
            {(n) => <Session server={h().server} name={name} how={w.tvSignInHow} onJoined={joined} onDone={() => setRun(n + 1)} />}
          </Show>
          <div class="acts">
            <A class="btn" href="/sync/recover" data-f="signin:found-kit" data-autofocus>
              {w.useRecoveryKit}
            </A>
            <button class="btn" data-f="signin:cant" aria-expanded={asking()} onClick={() => setAsking(!asking())}>
              {w.cantUseAny}
            </button>
          </div>
          <Show when={asking()}>
            <div class="sync-ask" role="group" aria-label={w.startOver}>
              <p>{w.startOverWarning}</p>
              <div class="acts">
                <button class="btn danger" data-f="signin:start-over" aria-busy={t.busy() || undefined} onClick={() => void startOver()}>
                  {w.startOver}
                </button>
                <button class="btn" data-f="signin:start-over-cancel" onClick={() => setAsking(false)}>
                  {syncWords.cancel}
                </button>
              </div>
            </div>
          </Show>
          <SignInState said={t.said()} fkey="signin:found" onAct={(a) => t.act(a)} />
        </Frame>
      )}
    </Show>
  );
}

/** A TV's first run (4.2): the pairing code for a phone to scan, its six digits after approval;
 * Use without an account leads to the doors, and a recovery kit stays for households with no
 * phone running Marquee. A TV that joined skips the doors. */
export function TvSignIn(props: { doors: () => JSX.Element }) {
  const navigate = useNavigate();
  const server = signInServer();
  const [run, setRun] = createSignal(1);
  return (
    <Show when={server && sync.settled() && sync.mayPair()} fallback={props.doors()}>
      <section class="step-body" aria-label={w.tvSignIn}>
        <h1>{w.tvSignIn}</h1>
        <Show when={run()} keyed>
          {(n) => <Session server={server} name={sync.defaultDeviceName()} how={w.tvSignInHow} onJoined={() => void homeAfterPull(navigate)} onDone={() => setRun(n + 1)} />}
        </Show>
        <div class="acts">
          <A class="btn" href="/welcome?step=doors" data-f="signin:tv-without" data-autofocus>
            {w.tvWithoutAccount}
          </A>
          <A class="btn" href="/sync/recover" data-f="signin:tv-kit">
            {w.useRecoveryKit}
          </A>
        </div>
      </section>
    </Show>
  );
}
