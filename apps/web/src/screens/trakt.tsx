/** Trakt (plans/m7-trakt.md section 3): connect with a code shown here and
 * approved on trakt.tv, then the connected card (who, when it last synced,
 * what waits, what Trakt could not name) with Sync now and Sign out. Each
 * state the core reports has its own words and its one way on. The record
 * lives in src/core/trakt.ts and is never shown. */

import { A } from "@solidjs/router";
import CircleCheck from "lucide-solid/icons/circle-check";
import Fingerprint from "lucide-solid/icons/fingerprint-pattern";
import Link from "lucide-solid/icons/link";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import LogOut from "lucide-solid/icons/log-out";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import { createSignal, For, type JSX, Match, onCleanup, Show, Switch } from "solid-js";
import { Qr } from "../components/qr";
import { liveView, reasonOf } from "../core/client";
import type { TraktLoginSnapshot } from "../core/generated/TraktLoginSnapshot";
import type { TraktStatus } from "../core/generated/TraktStatus";
import { remembered } from "../core/sync";
import { traktLink, traktLockedHere, traktSignOut, traktStatus, traktStorage, traktSync } from "../core/trakt";
import { form } from "../form";
import { fill, loginLine, loginRetry, pendingLine, stateCard, syncedLine, timeLeft, type TraktAction, traktWords as w } from "../lib/trakt";
import "./browse.css";
import "./settings.css";
import "./sync.css";
import "./trakt.css";

const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

function Said(props: { tone: "ok" | "warn" | "bad" | ""; children: JSX.Element }) {
  return (
    <p class={`st ${props.tone}`} role="status">
      <i class="dot" />
      {props.children}
    </p>
  );
}

const tones: Record<TraktStatus["state"], "ok" | "warn" | "bad" | ""> = {
  connected: "ok",
  failing: "warn",
  reconnect: "bad",
  off: "",
  thisDeviceOff: "warn",
  linking: "warn",
  offEverywhere: "",
  notConfigured: "",
};

export default function TraktScreen() {
  // Each Connect is a new code: a fresh traktLogin view.
  const [attempt, setAttempt] = createSignal(0);
  return (
    <main class="browse page settings-page trakt-page" data-vscroll>
      <h1>{w.title}</h1>
      <p class="lede">{w.lede}</p>
      <StorageNote />
      <Show when={traktLink} fallback={<Said tone="bad">{w.plainLink}</Said>}>
      <Show when={traktStatus()} fallback={<Said tone="warn">{w.loading}</Said>}>
        {(s) => (
          <Show when={attempt() > 0} fallback={<Card status={s()} connect={() => setAttempt((n) => n + 1)} />}>
            {/* Keyed by the attempt: a new code is a new sign-in view. */}
            <For each={[attempt()]}>{() => <Connect again={() => setAttempt((n) => n + 1)} close={() => setAttempt(0)} />}</For>
          </Show>
        )}
      </Show>
      </Show>
    </main>
  );
}

/** The connected card, or what each other state says and offers. */
function Card(props: { status: TraktStatus; connect: () => void }) {
  const card = () => stateCard(props.status, clock, traktLockedHere());
  const [busy, setBusy] = createSignal<"sync" | "signOut" | null>(null);
  const [said, setSaid] = createSignal<{ tone: "ok" | "bad"; text: string } | null>(null);
  const [asking, setAsking] = createSignal(false);
  const sync = async () => {
    if (busy()) return;
    setBusy("sync");
    setSaid(null);
    try {
      setSaid({ tone: "ok", text: syncedLine(await traktSync()) });
    } catch (e) {
      setSaid({ tone: "bad", text: fill(w.syncFailed, { reason: reasonOf(e) }) });
    } finally {
      setBusy(null);
    }
  };
  // In a Momotus account the connection is the account's: signing out disconnects every device.
  const signOutLabel = () => (props.status.shared ? w.signOutShared : w.signOut);
  const signOut = async () => {
    if (busy()) return;
    setBusy("signOut");
    setSaid(null);
    // The card says the rest ("disconnected on every device"); the line only confirms.
    const done = props.status.shared ? w.disconnected : w.signedOut;
    try {
      // A sealed copy that stayed is said by the storage note instead.
      if (await traktSignOut()) setSaid({ tone: "ok", text: done });
      setAsking(false);
    } catch (e) {
      setSaid({ tone: "bad", text: fill(w.signOutFailed, { reason: reasonOf(e) }) });
    } finally {
      setBusy(null);
    }
  };
  const button = (a: TraktAction, first: boolean) => {
    switch (a) {
      case "connect":
      case "signInHere":
      case "signInAgain":
        return (
          <button class="btn primary" data-f="trakt:connect" data-autofocus={first || undefined} onClick={props.connect}>
            <Link aria-hidden />
            {w[a]}
          </button>
        );
      case "syncNow":
        return (
          <button class="btn" classList={{ primary: first }} data-f="trakt:sync" data-autofocus={first || undefined} aria-busy={busy() === "sync" || undefined} onClick={() => void sync()}>
            <Show when={busy() === "sync"} fallback={<RefreshCw aria-hidden />}>
              <LoaderCircle aria-hidden class="spin" />
            </Show>
            {busy() === "sync" ? w.syncing : w.syncNow}
          </button>
        );
      case "unlock":
        return (
          <A class="btn primary" href="/sync" data-f="trakt:unlock" data-autofocus={first || undefined}>
            <Fingerprint aria-hidden />
            {w.unlockToUse}
          </A>
        );
      case "signOut":
        return (
          <button class="btn" data-f="trakt:sign-out" onClick={() => setAsking(true)}>
            <LogOut aria-hidden />
            {signOutLabel()}
          </button>
        );
    }
  };
  const connectedish = () => props.status.state === "connected" || props.status.state === "failing" || props.status.state === "reconnect";
  return (
    <>
      <section class="set trakt-card" aria-label={w.title} data-state={props.status.state}>
        <div class="trakt-state">
          <Said tone={tones[props.status.state]}>{card().line}</Said>
          <Show when={card().detail}>{(d) => <p class="note small">{d()}</p>}</Show>
        </div>
        <Show when={connectedish()}>
          <dl class="trakt-facts">
            <div>
              <dt>{props.status.lastOkMs ? fill(w.lastSynced, { time: clock(props.status.lastOkMs) }) : w.neverSynced}</dt>
            </div>
            <Show when={pendingLine(props.status.pendingWrites)}>
              {(p) => (
                <div>
                  <dt>{p()}</dt>
                </div>
              )}
            </Show>
            <Show when={props.status.importing}>
              {(i) => (
                <div class="trakt-import">
                  <dt>{fill(w.importing, { done: i().done, total: i().total })}</dt>
                  <dd>
                    <progress max={Math.max(1, i().total)} value={i().done} aria-label={w.importLabel} />
                  </dd>
                </div>
              )}
            </Show>
          </dl>
          <Show when={!traktStorage()}>
            <p class="note small">{remembered() ? w.keptSealed : w.keptInTab}</p>
          </Show>
        </Show>
        <Show when={said()}>{(s) => <Said tone={s().tone}>{s().text}</Said>}</Show>
        <Show
          when={!asking()}
          fallback={
            <div class="trakt-ask">
              <p class="note">{props.status.shared ? w.signOutSharedAsk : w.signOutAsk}</p>
              <div class="acts">
                <button class="btn danger" data-f="trakt:sign-out-confirm" data-autofocus aria-busy={busy() === "signOut" || undefined} onClick={() => void signOut()}>
                  <LogOut aria-hidden />
                  {busy() === "signOut" ? w.signingOut : signOutLabel()}
                </button>
                <button class="btn" data-f="trakt:sign-out-cancel" onClick={() => setAsking(false)}>
                  {w.cancel}
                </button>
              </div>
            </div>
          }
        >
          <Show
            when={card().actions.length}
            fallback={
              <div class="acts">
                <A class="btn" href="/settings" data-f="trakt:back" data-autofocus>
                  {w.backToSettings}
                </A>
              </div>
            }
          >
            <div class="acts">
              <For each={card().actions}>{(a, i) => button(a, i() === 0)}</For>
            </div>
          </Show>
        </Show>
      </section>
      <Show when={props.status.unmapped.length}>
        <section class="set">
          <h2>{w.unmappedTitle}</h2>
          <p class="note small sync-lead">{w.unmappedLede}</p>
          <ul class="trakt-unmapped">
            <For each={props.status.unmapped}>
              {(u) => (
                <li data-item={u.itemId}>
                  <TriangleAlert aria-hidden />
                  <span>
                    <b dir="auto">{u.name}</b>
                    <small>{u.reason}</small>
                  </span>
                </li>
              )}
            </For>
          </ul>
        </section>
      </Show>
    </>
  );
}

/** A sign-in: the code to enter at Trakt's page, until it is approved, refused or runs out. */
function Connect(props: { again: () => void; close: () => void }) {
  const view = liveView<TraktLoginSnapshot>("traktLogin");
  const login = () => view.state.data;
  const [now, setNow] = createSignal(Date.now());
  const tick = setInterval(() => setNow(Date.now()), 1000);
  onCleanup(() => clearInterval(tick));
  const code = () => {
    const l = login();
    return l?.phase === "code" ? l : null;
  };
  return (
    <section class="set trakt-connect">
      <Switch fallback={<Said tone="warn">{w.starting}</Said>}>
        <Match when={view.state.status.state === "failed" && view.state.status}>
          {(f) => (
            <>
              <Said tone="bad">{fill(w.failed, { reason: (f() as { reason: string }).reason })}</Said>
              <div class="acts">
                <button class="btn primary" data-f="trakt:again" data-autofocus onClick={props.again}>
                  <RefreshCw aria-hidden />
                  {w.newCode}
                </button>
                <button class="btn" data-f="trakt:cancel" onClick={props.close}>
                  {w.cancel}
                </button>
              </div>
            </>
          )}
        </Match>
        <Match when={code()}>
          {(c) => (
            <div class="pair-grid trakt-code">
              <Show when={form() !== "phone"}>
                <div class="qrbox">
                  <Qr text={c().qr} label={w.scanOrOpen} />
                </div>
              </Show>
              <div class="pair-steps">
                <p class="step">
                  {form() === "phone" ? w.open : w.scanOrOpen}{" "}
                  <a href={c().url} target="_blank" rel="noopener noreferrer" data-f="trakt:url">
                    {c().url.replace(/^https?:\/\//, "")}
                  </a>{" "}
                  {w.enterCode}
                </p>
                <p class="code" aria-hidden="true">
                  {c().userCode}
                </p>
                {/* Announced once, when the code shows; the time left is not a live region. */}
                <p class="sr" role="status">{`Code ${c().userCode.split("").join(" ")}`}</p>
                <p class="st warn">
                  <i class="dot" />
                  <span>
                    {w.waiting} {fill(w.goodFor, { time: timeLeft(c().expiresMs, now()) })}.
                  </span>
                </p>
                <div class="acts">
                  <button class="btn" data-f="trakt:cancel" data-autofocus onClick={props.close}>
                    {w.cancel}
                  </button>
                </div>
              </div>
            </div>
          )}
        </Match>
        <Match when={login()?.phase === "done"}>
          <Said tone="ok">{loginLine(login() as TraktLoginSnapshot)}</Said>
          <div class="acts">
            <button class="btn primary" data-f="trakt:done" data-autofocus onClick={props.close}>
              <CircleCheck aria-hidden />
              {w.close}
            </button>
          </div>
        </Match>
        <Match when={login() && login()?.phase !== "starting" && login()}>
          {(l) => (
            <>
              <Said tone={l().phase === "notConfigured" ? "" : "bad"}>{loginLine(l())}</Said>
              <div class="acts">
                <Show when={loginRetry(l())}>
                  <button class="btn primary" data-f="trakt:again" data-autofocus onClick={props.again}>
                    <RefreshCw aria-hidden />
                    {w.newCode}
                  </button>
                </Show>
                <button class="btn" data-f="trakt:cancel" data-autofocus={!loginRetry(l()) || undefined} onClick={props.close}>
                  {w.cancel}
                </button>
              </div>
            </>
          )}
        </Match>
      </Switch>
    </section>
  );
}

/** What this browser's storage did wrong with the Trakt sign-in, at the top, with its retry. */
function StorageNote() {
  const [busy, setBusy] = createSignal(false);
  const retry = async (run: () => Promise<void>) => {
    if (busy()) return;
    setBusy(true);
    await run().catch(() => undefined);
    setBusy(false);
  };
  return (
    <Show when={traktStorage()}>
      {(p) => (
        <div class="sync-note bad trakt-storage" role="alert">
          <TriangleAlert aria-hidden />
          <span>{p().words}</span>
          <button class="btn" data-f="trakt:storage-retry" aria-busy={busy() || undefined} onClick={() => void retry(p().retry)}>
            {p().words === w.saveFailed ? w.saveAgain : w.tryAgain}
          </button>
        </div>
      )}
    </Show>
  );
}
