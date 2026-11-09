/** Sign-in on the web (plans/accounts.md 4.1, 4.2, 5): the first screen with the providers the
 * sync service offers, the other ways, a sync server of one's own, Create your Marquee account
 * after `new`, the pairing code after `found` with Start over, and the TV's "Sign in with your
 * phone". Built from the welcome steps' own pieces; every word comes from
 * `apps/shared/account-words.json` (lib/account.ts). */

import { A, Navigate, useNavigate, useSearchParams } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ChevronRight from "lucide-solid/icons/chevron-right";
import LifeBuoy from "lucide-solid/icons/life-buoy";
import Mail from "lucide-solid/icons/mail";
import QrCode from "lucide-solid/icons/qr-code";
import Server from "lucide-solid/icons/server";
import { createEffect, createResource, createSignal, For, type JSX, on, onCleanup, onMount, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { markSrc } from "../components/marks";
import { intoHome, Sheet, SheetHead, WallPage } from "../components/wall";
import { askWho, profiles } from "../core/profiles";
import { type EmailFlow, emailCode, emailStart, held, openSignInWindow, relayProviders, runSignIn, type SignInEnd, type SignInRun, setHeld, signInServer } from "../core/sign-in";
import * as sync from "../core/sync";
import { pushBack } from "../focus/back";
import { autoFocusSoon } from "../focus/dom";
import { form } from "../form";
import {
  type Act,
  codeDigits,
  continueWith,
  continueWithEmail,
  EMAIL,
  emailEnds,
  emailFailure,
  emailFieldError,
  type Failure,
  fill,
  pairingStarting,
  providerName,
  providerOrder,
  resendIn,
  signInFailure,
  accountWords as w,
} from "../lib/account";
import { firstRunWords as fw, nextStep } from "../lib/first-run";
import { kidsSession } from "../lib/kids";
import { mayMakeKit } from "../lib/sync";
import { plainAddress, syncWords } from "../lib/words";
import { goBack } from "../focus/dom";
import { Session } from "./pairing";
import "./browse.css";
import "./settings.css";
import "./onboarding.css";

type From = "welcome" | "account";
const fromOf = (q: { from?: string | string[] }): From => (q.from === "welcome" ? "welcome" : "account");
const tail = (from: From) => (from === "welcome" ? "?from=welcome" : "");
/** Where the first screen is: first run's Login sheet, or its own page from Account. */
const firstScreen = (from: From) => (from === "welcome" ? "/welcome?step=login" : "/sign-in");
/** First run's Make your first profile, after Not now or a made account. */
const PROFILE_STEP = "/welcome?step=profile";
/** After creating: the recovery kit step (the web has no account passkey, D7), then Make your first profile on a
 * first run. A TV, or a profile that may not make kits (D17), goes straight on. Every target replaces the sign-in
 * entry, so Back from the profile form cannot reach Login. */
const afterCreate = (from: From) =>
  form() === "tv" || !mayMakeKit(profiles()) ? (from === "welcome" ? PROFILE_STEP : "/sync") : `/sync/kit?first=1${from === "welcome" ? "&then=profile" : ""}`;
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

/** Every sign-in page is a sheet over the still wall, on first run and from Settings' Sign in alike
 * (plans/first-screens.md 3.3, D10). */
function Page(props: { label: string; size?: "login" | "narrow"; children: JSX.Element }) {
  onMount(() => autoFocusSoon(() => document.querySelector<HTMLElement>(".sheet [data-autofocus]")));
  return (
    <WallPage>
      <Sheet size={props.size ?? "narrow"} label={props.label} busy={pulling()}>
        <UnlessPulling title={props.label}>{props.children}</UnlessPulling>
      </Sheet>
    </WallPage>
  );
}

/** A sign-in page with its own heading. */
const Frame = Page;

/** A later sign-in stage in its sheet: the heading, its line, then the stage. */
function Stage(props: { title: string; lead?: string; children: JSX.Element }) {
  return (
    <>
      <SheetHead title={props.title} />
      <Show when={props.lead}>
        <p class="sub">{props.lead}</p>
      </Show>
      {props.children}
    </>
  );
}

/** The live QR beside the ways in, on wide forms, as the sheet opens (the core renews the code while it shows): "Sign
 * in with your phone", how, and that it waits. One per page, since a page holds one pairing; `false` where this
 * browser may not pair. */
const cornerShows = () => form() === "desktop" && sync.mayPair();

function PhoneCorner(props: { server: string; how?: string; onJoined: (by: string | null) => void }) {
  return (
    <aside class="qrc" id="sign-in-qr" tabindex="-1" aria-labelledby="sign-in-qr-title">
      <h2 id="sign-in-qr-title">{fw.qrTitle}</h2>
      <Session server={props.server} name={sync.defaultDeviceName()} how={props.how ?? fw.qrHow} onJoined={props.onJoined} />
      <Waiting />
    </aside>
  );
}

/** "Waiting for your phone", its spinner still for reduced motion. */
const Waiting = () => (
  <p class="wait">
    <i class="spin" aria-hidden="true" />
    <span>{fw.qrWaiting}</span>
  </p>
);

/** "I already use Momotus on another device" beside a corner: the corner's code is that way in. */
function toCorner() {
  const corner = document.getElementById("sign-in-qr");
  corner?.focus({ preventScroll: true });
  corner?.scrollIntoView({ block: "nearest" });
}

/** Google's and Facebook's buttons by their own rules, on every form (design/marks; round2.html combo A). */
const branded = (p: string): "google" | "facebook" | null => (p === "google" || p === "facebook" ? p : null);

// The first pull after a join is under way (up to 15 s): the sheet says so, and nothing in it can be pressed.
const [pulling, setPulling] = createSignal(false);

/** A sheet's content, or "Getting your profiles" while the pull after a join runs. */
function UnlessPulling(props: { title: string; children: JSX.Element }) {
  return (
    <Show
      when={!pulling()}
      fallback={
        <>
          <SheetHead title={props.title} />
          <p class="wait pulling" role="status">
            <i class="spin" aria-hidden="true" />
            <span>{fw.gettingProfiles}</span>
          </p>
        </>
      }
    >
      {props.children}
    </Show>
  );
}

/** How the device joined, for Who's watching's line. */
export type Via = { provider?: string; by?: string | null };
const whoLineFor = (via: Via) =>
  via.provider ? fill(fw.whoProvider, { provider: providerName(via.provider) }) : via.by ? fill(fw.whoDevice, { device: via.by }) : fw.whoSignedIn;

/** After a join, once the first pull is in (so Home opens with the account's addons): Home as the root, or Who's
 * watching? over the wall when the account has two or more profiles or its one profile has a PIN (D2). */
export async function homeAfterPull(navigate: (to: string, o?: { replace?: boolean }) => void, via: Via = {}) {
  // A first round lost to a blip goes once more; Home opens after 15 s at most either way.
  const pull = async () => {
    await sync.syncNow();
    if (sync.problem() && !sync.report()) await sync.syncNow();
  };
  setPulling(true);
  await Promise.race([pull(), new Promise((done) => setTimeout(done, 15_000))]);
  intoHome(navigate);
  // Kept through the fade, so the actions never come back under it.
  setTimeout(() => setPulling(false), 400);
  const s = profiles();
  if (!s) return;
  const only = s.profiles.length === 1 ? s.profiles[0] : undefined;
  const step = nextStep({
    fresh: false,
    kidsSession: kidsSession(s),
    server: true,
    inAccount: true,
    profileStepDone: s.profileStepDone !== false,
    profiles: s.profiles.length,
    pin: only?.hasPin === true,
    locked: s.locked,
    after: "joined",
  });
  if (step === "who") askWho(whoLineFor(via));
}

/** The Login sheet (plans/first-screens.md 3.3): "Sign in" with Not now at its right, Continue with each provider the
 * sync service offers in its own marks (none: Create and I already use Momotus at once), email, Other ways to sign
 * in; wide forms show the live QR to the right, the phone a Use another device button (first after "I already use
 * Momotus"). Back, Escape and the grabber go back to Welcome on first run; from Account, Escape is Not now. */
export function SignInStart(props: { from: From; returning?: boolean }) {
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
  const leave = () => navigate(props.from === "welcome" ? PROFILE_STEP : "/sync");
  // From Account, Escape is Not now (its address's own Back would go Home); on first run it is Back, to Welcome.
  if (props.from === "account") onCleanup(pushBack(leave));
  const corner = cornerShows();
  // The phone's code in the sheet, after Use another device.
  const [pairing, setPairing] = createSignal(false);
  // The corner once the sync service answered with a way to sign in here.
  const offered = () => providers.state === "ready" && !none();
  const act = (a: Act) => {
    if (a === "retry") s.again();
    else if (a === "other") navigate(`/sign-in/other${tail(props.from)}`);
    else if (a === "notNow") leave();
    else s.setFailure(null);
  };
  const none = () => providers.state === "ready" && providers().length === 0;
  const buttons = () => (providers.state === "ready" ? providerOrder(providers()) : []);
  // Never on a TV (E13): typing an address and a code with a remote is the slow path the QR replaced.
  const email = () => form() !== "tv" && providers.state === "ready" && providers().includes(EMAIL);
  const device = () => form() === "phone" && offered() && sync.mayPair();
  const another = () => (
    <Show when={device()}>
      <button class="btn" data-f="signin:device" aria-expanded={pairing()} data-autofocus={props.returning ? "" : undefined} onClick={() => setPairing(!pairing())}>
        <QrCode aria-hidden />
        {fw.useAnotherDevice}
      </button>
    </Show>
  );
  return (
    <Show when={server} fallback={<Navigate href={props.from === "welcome" ? PROFILE_STEP : "/sync"} />}>
      <Sheet size="login" labelledby="sign-in-q" busy={providers.loading || pulling()} onGrab={props.from === "welcome" ? goBack : leave}>
        <UnlessPulling title={w.signInTitle}>
        <SheetHead id="sign-in-q" title={w.signInTitle}>
          <button class="flink" data-f="signin:not-now" onClick={leave}>
            {w.notNow}
          </button>
        </SheetHead>
        <p class="sub">{props.from === "account" ? w.libraryAdded : props.returning ? fw.loginReturning : fw.loginLine}</p>
        <Show
          when={providers.state !== "errored"}
          fallback={<FailureNote failure={signInFailure("unreachable") as Failure} fkey="signin:relay" onAct={(a) => (a === "retry" ? void refetch() : leave())} />}
        >
          <Show when={none()}>
            <Create server={server} from={props.from} />
          </Show>
          <div class="login-cols">
            <div class="prov" data-fgroup="providers">
              <Show when={props.returning}>{another()}</Show>
              <For each={buttons()}>
                {(p, i) => (
                  <button
                    class="btn"
                    classList={{ primary: i() === 0 && !branded(p) }}
                    data-f={`signin:${p}`}
                    data-provider={p}
                    data-brand={branded(p) ?? undefined}
                    data-autofocus={i() === 0 && !(props.returning && device()) ? "" : undefined}
                    onClick={() => s.go(p)}
                  >
                    <Show when={branded(p)} fallback={<span class="provider-mark" aria-hidden="true" />}>
                      {(b) => <img class="provider-mark" src={markSrc(b())} alt="" />}
                    </Show>
                    {continueWith(p)}
                  </button>
                )}
              </For>
              <Show when={email()}>
                <button
                  class="btn"
                  classList={{ primary: buttons().length === 0 }}
                  data-f="signin:email"
                  data-autofocus={buttons().length === 0 ? "" : undefined}
                  onClick={() => {
                    resume = null;
                    navigate(emailPath("address", props.from));
                  }}
                >
                  <Mail aria-hidden />
                  {continueWithEmail}
                </button>
              </Show>
              <Show when={!props.returning}>{another()}</Show>
              <Show when={pairing() && device()}>
                <Session server={server} name={sync.defaultDeviceName()} how={w.phoneScanHow} onJoined={(by) => void homeAfterPull(navigate, { by })} />
              </Show>
              <SignInState said={s.said()} fkey="signin" onAct={act} />
              <A class="flink" href={`/sign-in/other${tail(props.from)}`} data-f="signin:other">
                {w.otherWays}
              </A>
            </div>
            <Show when={corner && offered()}>
              <PhoneCorner server={server} onJoined={(by) => void homeAfterPull(navigate, { by })} />
            </Show>
          </div>
        </Show>
        </UnlessPulling>
      </Sheet>
    </Show>
  );
}

/** `/sign-in`, from Account on a device that used Not now: the same Login sheet over the wall (D10). */
export default function SignInScreen() {
  const [q] = useSearchParams();
  return (
    <Show when={!sync.joined()} fallback={<Navigate href="/sync" />}>
      <WallPage label={w.signInTitle}>
        <SignInStart from={fromOf(q)} />
      </WallPage>
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

type Taken = Extract<SignInEnd, { phase: "found" | "new" }>;
export type MailStage = "address" | "code";
interface MailHooks {
  /** The stage on screen. */
  stage: () => MailStage;
  /** A code was mailed, or the open flow's code stage is shown again. */
  sent: () => void;
  /** The code's answer. */
  taken: (end: Taken) => void;
}
const unbound: MailHooks = { stage: () => "code", sent: () => undefined, taken: () => undefined };

/** The email screen's state (plans/accounts-email.md 4): the address and the newest flow, in
 * memory only (E12). Every failure stays on the stage it came from, and Try again repeats that
 * stage's own call. The screen that shows it binds its stage and what an answer does. */
export function createMail(server: () => string) {
  const [address, setAddress] = createSignal("");
  const [code, setCode] = createSignal("");
  const [fieldError, setFieldError] = createSignal<string | null>(null);
  const [failure, setFailure] = createSignal<Failure | null>(null);
  const [busy, setBusy] = createSignal(false);
  /** The code's flow ended (`codeExpired`, `emailLimit`): the field stays, read-only, until a new code. */
  const [ended, setEnded] = createSignal(false);
  const [resent, setResent] = createSignal(false);
  /** The address the newest code went to, and when: Send a new code waits 60 s from it. */
  const [sentTo, setSentTo] = createSignal("");
  const [sentAt, setSentAt] = createSignal(0);
  let flow: EmailFlow | null = null;
  let again: (() => void) | null = null;
  let hooks = unbound;
  /** The newest call: an answer to an older one (left by Back, Use another address or Cancel) is dropped. */
  let epoch = 0;
  const failed = (code: string) => {
    const onCode = hooks.stage() === "code";
    const line = emailFieldError(code);
    if (line) {
      setFieldError(line);
      if (onCode) setCode("");
      return;
    }
    setFailure(emailFailure(code));
    if (onCode && emailEnds(code)) {
      setEnded(true);
      flow = null;
    }
  };
  /** Runs one call at a time; `work` answers what its result does, applied only while it is the newest. */
  const call = (work: () => Promise<() => void>) => {
    if (busy()) return;
    again = () => call(work);
    const mine = ++epoch;
    setFailure(null);
    setFieldError(null);
    setBusy(true);
    work().then(
      (apply) => {
        if (mine !== epoch) return;
        setBusy(false);
        apply();
      },
      (e: unknown) => {
        if (mine !== epoch) return;
        setBusy(false);
        failed(codeOf(e));
      },
    );
  };
  const mail = (resend: boolean) => {
    const typed = resend ? sentTo() : address().trim();
    call(async () => {
      setResent(false);
      const started = await emailStart(server(), typed);
      return () => {
        flow = started;
        setSentTo(typed);
        setSentAt(Date.now());
        setCode("");
        setEnded(false);
        setResent(resend);
        hooks.sent();
      };
    });
  };
  const submit = () => {
    const f = flow;
    const typed = code();
    if (!f || busy() || ended() || typed.length !== 6) return;
    call(async () => {
      const end = await emailCode(f.flow, typed);
      return () => {
        flow = null;
        hooks.taken(end);
      };
    });
  };
  /** The address stage's start: no line, no code. */
  const reset = () => {
    setCode("");
    setFieldError(null);
    setFailure(null);
    setEnded(false);
  };
  return {
    address,
    setAddress,
    code,
    fieldError,
    failure,
    busy,
    ended,
    resent,
    sentTo,
    sentAt,
    reset,
    submit,
    /** The code stage has something to show: an open flow, one that ended with its words, or a mail on its way. */
    hasFlow: () => flow !== null || ended() || busy(),
    /** Leaves the call under way: its answer, whatever it is, is dropped. */
    cancel: () => {
      epoch++;
      setBusy(false);
    },
    bind: (h: MailHooks | null) => {
      hooks = h ?? unbound;
    },
    /** Send code. The same address while its flow is open goes back to the code stage without a new mail. */
    send: () => {
      if (busy()) return;
      if (flow && sentTo() === address().trim() && flow.expiresMs > Date.now()) {
        reset();
        hooks.sent();
      } else mail(false);
    },
    /** Send a new code, to the address the last one went to. */
    resend: () => mail(true),
    /** Typed into the code field: digits only, cut at 6; the sixth digit sends it once. */
    type: (typed: string) => {
      if (busy() || ended()) return;
      const digits = codeDigits(typed);
      if (digits === code()) return;
      setCode(digits);
      if (digits.length === 6) submit();
    },
    /** Try again: this screen's last call again, the same address or the same code. */
    retry: () => again?.(),
    /** A held ticket ran out: a new code to the address in memory, at the code stage; with none
     * (the tab was reloaded), the address stage, empty. Answers the stage to show. */
    reopen: (): MailStage => {
      if (!sentTo()) {
        reset();
        return "address";
      }
      setAddress(sentTo());
      flow = null;
      reset();
      mail(true);
      return "code";
    },
    dismiss: () => setFailure(null),
  };
}
export type MailState = ReturnType<typeof createMail>;

/** The sign-in screen's email state, kept for the tab: a held ticket's Continue reuses its address. */
const signInMail = createMail(signInServer);
/** A held ticket's work, waiting for the new code's ticket, and the page it goes back to. */
let resume: { path: string; work: Pending | null; account: string | null } | null = null;
/** That work, handed to the page once it is back. */
let resumed: Pending | null = null;
/** The code stage was pushed over the address stage, so Use another address is Back. */
let pushedCode = false;

/** Each stage is a history entry; the address never is part of it. */
function emailPath(stage: MailStage, from: From) {
  const q = new URLSearchParams();
  if (stage === "code") q.set("stage", "code");
  if (from === "welcome") q.set("from", "welcome");
  const s = q.toString();
  return `/sign-in/email${s ? `?${s}` : ""}`;
}

/** The two stages of the email screen: the address, then the 6-digit code. */
export function EmailStages(props: { mail: MailState; stage: MailStage; fkey: string; heading?: "h1" | "h3"; onAct: (a: Act) => void; children?: JSX.Element }) {
  const m = props.mail;
  const k = (name: string) => `${props.fkey}:${name}`;
  const errorId = `${props.fkey.replace(/\W/g, "-")}-error`;
  let field: HTMLInputElement | undefined;
  // On a stage change and on a line under the field, focus goes to the stage's field.
  // The new stage's title is said once, as the focus lands in its field.
  const [said, setSaid] = createSignal("");
  createEffect(
    on(
      () => props.stage,
      (s) => {
        setSaid(s === "code" ? w.codeTitle : w.emailTitle);
        queueMicrotask(() => field?.focus());
      },
      { defer: true },
    ),
  );
  createEffect(on(m.fieldError, (e) => e && field?.focus(), { defer: true }));
  const [now, setNow] = createSignal(Date.now());
  const tick = setInterval(() => setNow(Date.now()), 1000);
  onCleanup(() => clearInterval(tick));
  // The tick may be older than the mail: never count from before it.
  const wait = () => resendIn(m.sentAt(), Math.max(now(), m.sentAt()));
  // The failure's own Send a new code stands in for the row's while it shows.
  const standing = () => props.stage === "code" && !m.failure()?.actions.some((a) => a.act === "resend");
  const fieldLine = () => (
    <Show when={m.fieldError()}>
      {(e) => (
        <p class="st bad" id={errorId} role="status">
          <i class="dot" />
          {e()}
        </p>
      )}
    </Show>
  );
  return (
    <>
      <Show
        when={props.stage === "code"}
        fallback={
          <>
            <Dynamic component={props.heading ?? "h1"}>{w.emailTitle}</Dynamic>
            <form
              class="install email-form"
              noValidate
              onSubmit={(e) => {
                e.preventDefault();
                m.send();
              }}
            >
              <label class="field-line">
                <span class="kicker">{w.emailLabel}</span>
                <input
                  ref={(el) => (field = el)}
                  type="email"
                  name="email"
                  autocomplete="email"
                  autocapitalize="off"
                  spellcheck={false}
                  value={m.address()}
                  onInput={(e) => m.setAddress(e.currentTarget.value)}
                  readOnly={m.busy()}
                  aria-busy={m.busy() || undefined}
                  enterkeyhint="send"
                  data-f={k("address")}
                  data-autofocus
                  aria-invalid={m.fieldError() ? true : undefined}
                  aria-describedby={m.fieldError() ? errorId : undefined}
                />
              </label>
              {fieldLine()}
              <button class="btn primary" type="submit" data-f={k("send")} aria-busy={m.busy() || undefined}>
                {w.emailSend}
              </button>
            </form>
          </>
        }
      >
        <Dynamic component={props.heading ?? "h1"}>{w.codeTitle}</Dynamic>
        <p class="lead">{fill(w.codeSentTo, { email: m.sentTo() })}</p>
        <form
          class="install email-form"
          noValidate
          onSubmit={(e) => {
            e.preventDefault();
            m.submit();
          }}
        >
          <label class="field-line">
            <span class="kicker">{w.codeLabel}</span>
            <input
              ref={(el) => (field = el)}
              type="text"
              name="code"
              inputmode="numeric"
              autocomplete="one-time-code"
              pattern="[0-9 -]*"
              spellcheck={false}
              value={m.code()}
              onInput={(e) => {
                m.type(e.currentTarget.value);
                e.currentTarget.value = m.code();
              }}
              readOnly={m.busy() || m.ended()}
              aria-busy={m.busy() || undefined}
              enterkeyhint="go"
              data-f={k("code")}
              data-autofocus
              aria-invalid={m.fieldError() ? true : undefined}
              aria-describedby={m.fieldError() ? errorId : undefined}
            />
          </label>
          {fieldLine()}
        </form>
        {/* Gmail files these mails as spam today (plans/accounts-email.md 3): always said, at normal weight. */}
        <p class="aside">{w.codeMissing}</p>
        <Show when={m.resent()}>
          <p class="st ok" role="status">
            <i class="dot" />
            {w.codeResent}
          </p>
        </Show>
      </Show>
      <Show when={m.failure()}>{(f) => <FailureNote failure={f()} fkey={props.fkey} onAct={props.onAct} />}</Show>
      <div class="acts">
        <Show when={standing()}>
          {/* Not a live region: the count is read once, and the line below says when it turns on. */}
          <button class="btn" data-f={k("new-code")} disabled={wait() > 0 || m.busy()} onClick={() => m.resend()}>
            {wait() > 0 ? fill(w.codeResendIn, { seconds: wait() }) : w.codeResend}
          </button>
          <button class="btn" data-f={k("another")} onClick={() => props.onAct("otherAddress")}>
            {w.codeOtherAddress}
          </button>
        </Show>
        {props.children}
      </div>
      <p class="sr" aria-live="polite">
        {standing() && m.sentAt() && wait() === 0 ? w.codeResend : ""}
      </p>
      <p class="sr" aria-live="polite">
        {said()}
      </p>
    </>
  );
}

/** `/sign-in/email`: Sign in with email, its stage in the query (`?stage=code`), never the
 * address. A code stage with no code mailed in this tab (a reload) is the address stage; a TV
 * goes to Sign in (E13). */
export function EmailSignIn() {
  const [q] = useSearchParams();
  const from = fromOf(q);
  const navigate = useNavigate();
  const stage = (): MailStage => (q.stage === "code" ? "code" : "address");
  const m = signInMail;
  m.bind({
    stage,
    sent: () => {
      if (stage() === "code") return;
      pushedCode = true;
      navigate(emailPath("code", from));
    },
    taken: (end) => {
      const r = resume;
      resume = null;
      setHeld({ server: signInServer(), provider: EMAIL, ticket: end.ticket, account: end.phase === "found" ? end.account : null });
      // Back to the page whose ticket ran out, unless the sign-in found an account it did not have.
      if (r && !(end.phase === "found" && !r.account)) {
        resumed = r.work;
        navigate(r.path, { replace: true });
      } else navigate(`/sign-in/${end.phase}${tail(from)}`);
    },
  });
  onCleanup(() => {
    m.cancel();
    m.bind(null);
  });
  createEffect(
    on(stage, (s) => {
      if (s === "address") {
        m.cancel();
        m.reset();
      } else if (!m.hasFlow()) navigate(emailPath("address", from), { replace: true });
    }),
  );
  const toAddress = () => {
    m.cancel();
    if (pushedCode) {
      pushedCode = false;
      navigate(-1);
    } else navigate(emailPath("address", from), { replace: true });
  };
  const act = (a: Act) => {
    if (a === "retry") m.retry();
    else if (a === "resend") m.resend();
    else if (a === "otherAddress") toAddress();
    else if (a === "other") navigate(firstScreen(from));
    else if (a === "notNow") navigate(from === "welcome" ? PROFILE_STEP : "/sync");
    else m.dismiss();
  };
  return (
    <Show when={form() !== "tv"} fallback={<Navigate href={firstScreen(from)} />}>
      <Show when={!sync.joined()} fallback={<Navigate href="/sync" />}>
        <Frame label={stage() === "code" ? w.codeTitle : w.emailTitle}>
          <EmailStages mail={m} stage={stage()} fkey="signin:email" onAct={act} />
        </Frame>
      </Show>
    </Show>
  );
}

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
  // Back from "Sign in again to finish" by email: the work waiting for the new code's ticket.
  onMount(() => {
    const work = resumed;
    resumed = null;
    if (work) void run(work);
  });
  const act = (a: Act) => {
    if (a === "retry") {
      const h = held();
      if ((!failure() || failed === "ticketExpired") && h?.provider === EMAIL) {
        // The code stage with the address still in memory and a new code (4.6), then this page again.
        resume = { path: location.pathname + location.search, work: pending, account: h.account };
        pushedCode = false;
        navigate(emailPath(signInMail.reopen(), from));
      } else if (!failure() || failed === "ticketExpired") again.again();
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

/** `new`: Create your Momotus account, or "I already use Momotus on another device": the pairing
 * code, then the sign-in is linked with the ticket held. */
export function NewAccount() {
  const [q] = useSearchParams();
  const from = fromOf(q);
  return (
    <Show when={held()} fallback={<Navigate href={firstScreen(from)} />}>
      {(h) => (
        <Page label={w.createAccount} size="login">
          <Stage title={w.createAccount} lead={from === "account" ? `${w.noAccountYet} ${w.libraryAdded}` : w.noAccountYet}>
            <Create server={h().server} from={from} />
          </Stage>
        </Page>
      )}
    </Show>
  );
}

/** Create, or join by the pairing code. With a sign-in held, the create takes its ticket and a
 * join links it; without one (a sync service that offers no provider) the create is the network's
 * sign-up (plans/tv-qr-login.md Q7) and a join links nothing. */
function Create(props: { server: string; from: From }) {
  const navigate = useNavigate();
  const name = sync.defaultDeviceName();
  const t = useHeldTicket(props.from);
  // Desktop's stage shows the code in its corner from the start, and "I already use" points there.
  const corner = cornerShows();
  const [pairing, setPairing] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [failure, setFailure] = createSignal<Failure | null>(null);
  const made = () => {
    sync.rememberJoin(props.server, name);
    setHeld(null);
    navigate(afterCreate(props.from), { replace: true });
  };
  const create = async () => {
    if (held()) {
      return t.run(async (ticket) => {
        await sync.createAccount(props.server, name, "", ticket);
        made();
      });
    }
    if (busy()) return;
    setBusy(true);
    setFailure(null);
    try {
      await sync.createAccount(props.server, name, "");
      made();
    } catch (e) {
      setFailure(signInFailure(codeOf(e)));
    } finally {
      setBusy(false);
    }
  };
  // After a provider sign-in the account is created at once, as any app's does; the choice stays only when that failed.
  onMount(() => {
    if (held()) void create();
  });
  const joined = (by: string | null) => {
    const h = held();
    if (!h) return void homeAfterPull(navigate, { by });
    void t.run(async (ticket) => {
      await sync.linkIdentity(ticket);
      setHeld(null);
      await homeAfterPull(navigate, { provider: h.provider, by });
    });
  };
  return (
    <>
      <Show
        when={pairing()}
        fallback={
          <div class="acts">
            <button class="btn primary" data-f="signin:create" data-autofocus aria-busy={t.busy() || busy() || undefined} onClick={() => void create()}>
              {w.createAccount}
            </button>
            <button class="btn" data-f="signin:already" aria-controls={corner ? "sign-in-qr" : undefined} onClick={() => (corner ? toCorner() : setPairing(true))}>
              <Show when={corner}>
                <QrCode aria-hidden />
              </Show>
              {w.alreadyUse}
            </button>
          </div>
        }
      >
        <Session server={props.server} name={name} how={form() === "phone" ? w.phoneScanHow : w.tvScanHow} onJoined={joined} />
      </Show>
      <Show when={failure()}>{(f) => <FailureNote failure={f()} fkey="signin:create" onAct={(a) => (a === "retry" ? void create() : a === "other" ? navigate(`/sign-in/other${tail(props.from)}`) : setFailure(null))} />}</Show>
      <SignInState said={t.said()} fkey="signin:new" onAct={(a) => t.act(a)} />
      {/* A create needs the slot the code holds: the code goes while it runs, and comes back if it fails. */}
      <Show when={corner && !t.busy() && !busy()}>
        <PhoneCorner server={props.server} onJoined={joined} />
      </Show>
    </>
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
  const joined = (by: string | null) => {
    const provider = held()?.provider;
    setHeld(null);
    void homeAfterPull(navigate, { provider, by });
  };
  return (
    <Show when={held()} fallback={<Navigate href={firstScreen(from)} />}>
      {(h) => (
        <Page label={w.signInTitle}>
          <Stage title={w.signInTitle} lead={w.foundWeb}>
          {/* Start over needs the account slot the code holds: asked, the code goes; Cancel makes a new one. */}
          <Show when={!asking()}>
            <Session server={h().server} name={name} how={w.tvScanHow} onJoined={joined} />
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
          </Stage>
        </Page>
      )}
    </Show>
  );
}

/** The TV's Login sheet (plans/first-screens.md 3.3, D3): the QR, three numbered steps and the waiting line, then
 * Not now (the first focus) and Other ways to sign in; the QR needs no press, and a join moves on by itself. With no
 * QR (no server, not settled, or this TV may not pair) one line says so with Try again and Not now. */
export function TvSignIn(props: { onNotNow: () => void }) {
  const navigate = useNavigate();
  const [server, setServer] = createSignal(signInServer());
  const [asking, setAsking] = createSignal(false);
  // Try again asks afresh: this browser's stored account, then the sync service it knows (if any); the focus stays
  // on Try again while the answer is the same.
  const retry = async () => {
    if (asking()) return;
    setAsking(true);
    await sync.vaultReady;
    const s = signInServer();
    if (s) await relayProviders(s).catch(() => null);
    setServer(s);
    setAsking(false);
    autoFocusSoon(() => document.querySelector<HTMLElement>('[data-f="signin:retry"], [data-f="signin:not-now"]'));
  };
  const qr = () => !!server() && sync.settled() && sync.mayPair();
  // No sync service (or none in reach): unreachable, with Try again. One this TV may not pair with: refused, Not now only.
  const unreachable = () => !server();
  const acts = (mode: "qr" | "retry" | "notNow") => (
    <div class="acts" data-fgroup="tv-login">
      <Show when={mode === "retry"}>
        <button class="btn" data-f="signin:retry" aria-busy={asking() || undefined} onClick={() => void retry()}>
          {fw.tryAgain}
        </button>
      </Show>
      <button class="btn" data-f="signin:not-now" data-autofocus onClick={() => props.onNotNow()}>
        {w.notNow}
      </button>
      <Show when={mode === "qr"}>
        <A class="btn" href="/sign-in/other?from=welcome" data-f="signin:other">
          {w.otherWays}
        </A>
      </Show>
    </div>
  );
  return (
    <Sheet size="tv" labelledby="sign-in-q" busy={pulling() || !sync.settled()}>
      <UnlessPulling title={fw.tvLoginTitle}>
        <Show
          when={qr()}
          fallback={
            <>
              <SheetHead id="sign-in-q" title={fw.tvLoginTitle} />
              {/* Before this browser has read its stored account, nothing is known yet: no error to show. */}
              <Show
                when={sync.settled()}
                fallback={
                  <p class="wait" role="status">
                    <i class="spin" aria-hidden="true" />
                    <span>{pairingStarting(false).text}</span>
                  </p>
                }
              >
                <p class="sub">{fw.tvNoQr}</p>
                <p class="st bad" role="status">
                  <i class="dot" />
                  {unreachable() ? w.unreachable : w.signInRefused}
                </p>
              </Show>
              {acts(sync.settled() && unreachable() ? "retry" : "notNow")}
            </>
          }
        >
          <div class="tvl">
            <Session server={server()} name={sync.defaultDeviceName()} onJoined={(by) => void homeAfterPull(navigate, { by })} />
            <div class="tx">
              <h1 id="sign-in-q" tabindex="-1">
                {fw.tvLoginTitle}
              </h1>
              <p class="sub">{fw.loginLine}</p>
              <ol>
                <For each={fw.tvSteps}>{(s) => <li>{s}</li>}</For>
              </ol>
              <Waiting />
              {acts("qr")}
            </div>
          </div>
        </Show>
      </UnlessPulling>
    </Sheet>
  );
}
