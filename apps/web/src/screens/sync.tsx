/** Devices and sync (plans/m7-sync.md, plans/core-protocol.md): start an
 * account, join one with a recovery kit, see this device's state and the
 * others, add a device, make a kit, remove a device or report one lost,
 * compare device lists after the server changed, stop syncing, and remember
 * this browser with a passkey. The record lives in src/core/sync.ts; a kit
 * is shown once and kept nowhere. No key, token or kit reaches a log. */

import { A, useNavigate, useParams, useSearchParams } from "@solidjs/router";
import Camera from "lucide-solid/icons/camera";
import Check from "lucide-solid/icons/check";
import Copy from "lucide-solid/icons/copy";
import Eye from "lucide-solid/icons/eye";
import EyeOff from "lucide-solid/icons/eye-off";
import Fingerprint from "lucide-solid/icons/fingerprint-pattern";
import KeyRound from "lucide-solid/icons/key-round";
import LifeBuoy from "lucide-solid/icons/life-buoy";
import LogOut from "lucide-solid/icons/log-out";
import MonitorSmartphone from "lucide-solid/icons/monitor-smartphone";
import Plus from "lucide-solid/icons/plus";
import QrCode from "lucide-solid/icons/qr-code";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import ShieldAlert from "lucide-solid/icons/shield-alert";
import Trash from "lucide-solid/icons/trash";
import TriangleAlert from "lucide-solid/icons/triangle-alert";
import X from "lucide-solid/icons/x";
import { createEffect, createMemo, createResource, createSignal, For, type JSX, on, onCleanup, onMount, Show, untrack } from "solid-js";
import { privateLink } from "../core/client";
import type { MemberInfo } from "../core/generated/MemberInfo";
import * as sync from "../core/sync";
import { autoFocus } from "../focus/dom";
import { form } from "../form";
import {
  fingerprintGroups,
  forkDrop,
  forkKits,
  forkSides,
  kitsGoing,
  lostDrop,
  lostRows,
  memberName,
  sortMembers,
  sponsorName,
  watchForCode,
} from "../lib/sync";
import {
  addedLine,
  failedSince,
  forkButton,
  goesWith,
  lostButton,
  lostKits,
  named,
  outcomeLine,
  outcomeOf,
  plainAddress,
  removeDeviceAsk,
  reportNotes,
  type SyncStep,
  stepProblem,
  syncWords as w,
} from "../lib/words";
import { LinkRow } from "./settings";
import "./browse.css";
import "./settings.css";
import "./sync.css";

const tv = () => form() === "tv";
// Only the protocol's string codes: a DOMException carries a numeric `code` of its own.
const codeOf = (e: unknown) => {
  const c = e && typeof e === "object" && "code" in e ? (e as { code?: unknown }).code : undefined;
  return typeof c === "string" ? c : undefined;
};
const say = (e: unknown, step: SyncStep) => stepProblem(codeOf(e), e instanceof Error ? e.message : String(e), step);
const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
/** After the page changed under it, focus goes to the step's own target (remote and keys only). */
const focusSoon = (sel: string) => queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>(sel)));
/** A code read character by character to a screen reader; the screen shows it as is. */
const spelled = (text: string) => [...text.replace(/[\s-]/g, "")].join(" ");

function Said(props: { tone: "ok" | "warn" | "bad" | ""; children: JSX.Element; live?: boolean }) {
  return (
    <p class={`st ${props.tone}`} role={props.live === false ? undefined : "status"}>
      <i class="dot" />
      {props.children}
    </p>
  );
}

function Spoken(props: { text: string; class: string; label?: string }) {
  return (
    <>
      <p class={props.class} aria-hidden="true">
        {props.text}
      </p>
      <p class="sr">{`${props.label ? `${props.label}: ` : ""}${spelled(props.text)}`}</p>
    </>
  );
}

/** A row a TV cannot act on: focusable so the remote reads it, with the line that says where to do it. */
function ElsewhereRow(props: { icon: JSX.Element; title: string; text: string; fkey: string }) {
  return (
    <div class="link-row elsewhere" tabindex="0" data-f={props.fkey} aria-disabled="true">
      {props.icon}
      <span>
        <b>{props.title}</b>
        <small>{props.text}</small>
      </span>
    </div>
  );
}

function Field(props: {
  label: string;
  value: string;
  onInput: (v: string) => void;
  fkey: string;
  type?: string;
  placeholder?: string;
  mono?: boolean;
  hint?: string;
  /** A secret typed here is masked until the user shows it. */
  secret?: boolean;
}) {
  const [shown, setShown] = createSignal(false);
  return (
    <label class="field-line">
      <span class="kicker">{props.label}</span>
      <span class="field-row">
        <input
          type={props.secret && !shown() ? "password" : (props.type ?? "text")}
          classList={{ mono: props.mono }}
          placeholder={props.placeholder}
          value={props.value}
          onInput={(e) => props.onInput(e.currentTarget.value)}
          data-f={props.fkey}
          autocomplete="off"
          autocapitalize="off"
          autocorrect="off"
          spellcheck={false}
        />
        <Show when={props.secret}>
          <button
            type="button"
            class="btn reveal"
            data-f={`${props.fkey}:reveal`}
            aria-pressed={shown()}
            aria-label={shown() ? w.hideKit : w.showKit}
            onClick={(e) => {
              e.preventDefault();
              setShown(!shown());
            }}
          >
            <Show when={shown()} fallback={<Eye aria-hidden />}>
              <EyeOff aria-hidden />
            </Show>
          </button>
        </Show>
      </span>
      <Show when={props.hint}>
        <small class="field-hint">{props.hint}</small>
      </Show>
    </label>
  );
}

export default function SyncScreen() {
  return (
    <main class="browse page settings-page sync-page" data-vscroll>
      <h1>{w.title}</h1>
      <p class="lede">{w.lede}</p>
      <Show when={privateLink} fallback={<Said tone="bad">{w.plainLink}</Said>}>
        <Show when={!sync.locked()} fallback={<Locked />}>
          <Show when={sync.joined()} fallback={<NotJoined />}>
            <Joined />
          </Show>
        </Show>
      </Show>
    </main>
  );
}

/** This browser remembers an account: the passkey opens it. */
function Locked() {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const unlock = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await sync.unlock();
    } catch (e) {
      setError(say(e, "unlock"));
    } finally {
      setBusy(false);
    }
  };
  // The storage could not be read: it may hold an account, so the page waits for a check or a forget.
  const unreadable = () => sync.memoryProblem() === w.vaultUnreadable;
  return (
    <section class="set">
      <Show when={!unreadable()}>
        <p class="note sync-lead">{w.locked}</p>
      </Show>
      <Show when={sync.memoryProblem()}>{(p) => <Said tone="bad">{p()}</Said>}</Show>
      <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
      <div class="acts">
        <Show
          when={!unreadable()}
          fallback={
            <button class="btn primary" data-f="sync:recheck" data-autofocus onClick={() => void sync.recheckVault()}>
              <RefreshCw aria-hidden />
              {w.checkAgain}
            </button>
          }
        >
          <button class="btn primary" data-f="sync:unlock" data-autofocus aria-busy={busy() || undefined} onClick={() => void unlock()}>
            <Fingerprint aria-hidden />
            {busy() ? w.unlocking : w.unlock}
          </button>
        </Show>
        <button class="btn" data-f="sync:forget-browser" onClick={() => void sync.forgetBrowser().catch(() => undefined)}>
          {w.forgetBrowser}
        </button>
      </div>
    </section>
  );
}

function NotJoined() {
  return (
    <>
      <Show when={sync.problem()}>{(p) => <Said tone="bad">{p().words}</Said>}</Show>
      <Show when={!sync.problem() && sync.notice()}>{(n) => <Said tone="ok">{n()}</Said>}</Show>
      <section class="set">
        <div class="links">
          <LinkRow href="/pair" fkey="sync:pair" icon={<QrCode aria-hidden />} title={w.pairTitle} text={w.pairText} />
          <LinkRow href="/sync/start" fkey="sync:start" icon={<Plus aria-hidden />} title={w.startTitle} text={w.startText} />
          <LinkRow href="/sync/recover" fkey="sync:recover" icon={<LifeBuoy aria-hidden />} title={w.recoverTitle} text={w.recoverText} />
        </div>
        <p class="note small">{w.tabOnly}</p>
      </section>
    </>
  );
}

function Joined() {
  const navigate = useNavigate();
  const [listError, setListError] = createSignal<string | null>(null);
  const load = () =>
    sync.loadDevices().then(
      () => setListError(null),
      (e) => setListError(say(e, "load")),
    );
  onMount(() => void load());
  const now = async () => {
    await sync.syncNow();
    if (sync.joined()) void load();
  };
  const outcome = () => {
    const r = sync.report();
    return r ? outcomeOf(r) : null;
  };
  const notes = () => {
    const r = sync.report();
    return r ? reportNotes(r) : [];
  };
  const state = (): { tone: "ok" | "warn" | "bad" | ""; text: string } => {
    const p = sync.problem();
    if (p) return { tone: "bad", text: p.words };
    if (sync.syncing() && !sync.syncedAt()) return { tone: "warn", text: w.syncing };
    const at = sync.syncedAt();
    const o = outcome();
    if (!at || !o) return { tone: "", text: w.notSynced };
    return { tone: o === "complete" ? "ok" : o === "pending" ? "warn" : "bad", text: outcomeLine(o, clock(at)) };
  };
  // Read aloud only when the outcome or its reasons change, never for the minute ticking by.
  const spoken = () => {
    const p = sync.problem();
    if (p) return p.words;
    const o = outcome();
    if (!o) return "";
    return [o === "complete" ? "Synced." : o === "pending" ? "Synced. Some changes still wait." : "Synced. Some changes did not sync.", ...notes()].join(" ");
  };
  const members = () => sortMembers(sync.devices()?.members ?? []);
  const noKit = () => sync.devices() !== null && !members().some((m) => m.kit);
  const [asking, setAsking] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [said, setSaid] = createSignal<{ id: string; tone: "ok" | "bad"; text: string } | null>(null);
  const remove = async (m: MemberInfo) => {
    if (busy()) return;
    setBusy(true);
    try {
      await sync.removeDevice(m.id);
    } catch (e) {
      setSaid({ id: m.id, tone: "bad", text: say(e, "remove") });
      setBusy(false);
      return;
    }
    setBusy(false);
    setAsking(null);
    setSaid({ id: "list", tone: "ok", text: `${w.removed}: ${memberName(m)}` });
    focusSoon('[data-f="sync:now"]');
    await sync.loadDevices().catch(() => setSaid({ id: "list", tone: "ok", text: `${w.removed}: ${memberName(m)}. ${w.listStale}` }));
  };
  const ask = (id: string | null) => {
    setAsking(id);
    setSaid(null);
    focusSoon(id ? `[data-f="sync:${id}:cancel"]` : '[data-f="sync:now"]');
  };
  return (
    <>
      <StorageNote />
      <section class="set sync-card" aria-label={w.thisDevice} data-round={sync.rounds()} data-syncing={sync.syncing() ? "" : undefined}>
        <div class="sync-state">
          <Said tone={state().tone} live={false}>
            {state().text}
          </Said>
          <For each={notes()}>
            {(n) => (
              <p class="sync-note">
                <TriangleAlert aria-hidden />
                {n}
              </p>
            )}
          </For>
          <Show when={sync.devices()?.rotationPending}>
            <p class="sync-note">
              <TriangleAlert aria-hidden />
              {w.rotationPending}
            </p>
          </Show>
          <Show when={failedSince(sync.failed().refused, sync.failed().unreadable, sync.failed().newer)}>
            {(line) => (
              <div class="sync-note bad">
                <TriangleAlert aria-hidden />
                <span>{line()}</span>
                <button class="btn" data-f="sync:dismiss" onClick={sync.dismissFailed}>
                  {w.dismiss}
                </button>
              </div>
            )}
          </Show>
          <p class="sr" role="status" aria-live="polite">
            {spoken()}
          </p>
        </div>
        <div class="acts">
          <button class="btn primary" data-f="sync:now" data-autofocus aria-busy={sync.syncing() || undefined} onClick={() => void now()}>
            <RefreshCw aria-hidden classList={{ spin: sync.syncing() }} />
            {sync.syncing() ? w.syncing : w.syncNow}
          </button>
          <Show when={sync.problem()?.code === "fork"}>
            <A class="btn" href="/sync/fork" data-f="sync:server-list">
              <MonitorSmartphone aria-hidden />
              {w.useServer}
            </A>
          </Show>
        </div>
        <Show when={sync.fingerprint()}>
          {(fp) => (
            <div class="sync-compare">
              <span class="kicker">{w.compareLabel}</span>
              <Spoken class="sync-fp mono" text={fingerprintGroups(fp())} label={w.compareLabel} />
              <p class="note small">{w.compareHelp}</p>
            </div>
          )}
        </Show>
      </section>

      <Show when={sync.recovered() || noKit()}>
        <p class="fa-note sync-callout">
          <KeyRound aria-hidden />
          {sync.recovered() ? (tv() ? w.afterRecoverTv : w.afterRecover) : tv() ? w.noKitTv : w.noKit}
        </p>
      </Show>

      <section class="set">
        <h2>{w.devices}</h2>
        <Show when={said()?.id === "list" && said()}>{(s) => <Said tone={s().tone}>{s().text}</Said>}</Show>
        <Show when={listError()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
        <Show when={sync.devices()} fallback={<Show when={!listError()}>{<Said tone="warn">{w.loadingDevices}</Said>}</Show>}>
          <ul class="sync-members">
            <For each={members()}>
              {(m) => (
                <li class="sync-member" classList={{ me: m.thisDevice }} data-member={m.id}>
                  <div class="sync-mtext">
                    <h3 dir="auto">
                      {memberName(m)}
                      <Show when={m.thisDevice}>
                        <span class="sync-tag">{w.thisDevice}</span>
                      </Show>
                    </h3>
                    <p>{addedLine(m.kit, m.addedMs, sponsorName(m, members()))}</p>
                    <Show when={said()?.id === m.id && said()}>{(s) => <Said tone={s().tone}>{s().text}</Said>}</Show>
                  </div>
                  <Show when={!m.thisDevice}>
                    <Show
                      when={asking() === m.id}
                      fallback={
                        <div class="acts sync-macts">
                          <button class="btn" data-f={`sync:${m.id}:remove`} aria-label={`${w.remove} ${memberName(m)}`} onClick={() => ask(m.id)}>
                            <Trash aria-hidden />
                            {w.remove}
                          </button>
                          <Show when={!m.kit}>
                            <button class="btn" data-f={`sync:${m.id}:lost`} aria-label={`${w.reportLost}: ${memberName(m)}`} onClick={() => navigate(`/sync/lost/${m.id}`)}>
                              <ShieldAlert aria-hidden />
                              {w.reportLost}
                            </button>
                          </Show>
                        </div>
                      }
                    >
                      <div class="sync-ask" role="group" aria-label={`${w.remove} ${memberName(m)}`}>
                        <p>{m.kit ? w.removeKitAsk : removeDeviceAsk(memberName(m))}</p>
                        <Show when={sync.remembered()}>
                          <p class="note small">{w.passkeyAsk}</p>
                        </Show>
                        <div class="acts">
                          <button class="btn danger" data-f={`sync:${m.id}:confirm`} aria-busy={busy() || undefined} onClick={() => void remove(m)}>
                            <Trash aria-hidden />
                            {`${w.remove} ${memberName(m)}`}
                          </button>
                          <button class="btn" data-f={`sync:${m.id}:cancel`} onClick={() => ask(null)}>
                            {w.cancel}
                          </button>
                        </div>
                      </div>
                    </Show>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </section>

      <section class="set">
        <div class="links">
          <Show
            when={!tv()}
            fallback={
              <>
                <ElsewhereRow fkey="sync:add" icon={<QrCode aria-hidden />} title={w.addTitle} text={w.tvAdd} />
                <ElsewhereRow fkey="sync:kit" icon={<KeyRound aria-hidden />} title={w.kitTitle} text={w.tvKit} />
              </>
            }
          >
            <LinkRow href="/sync/add" fkey="sync:add" icon={<QrCode aria-hidden />} title={w.addTitle} text={w.addText} />
            <LinkRow href="/sync/kit" fkey="sync:kit" icon={<KeyRound aria-hidden />} title={w.kitTitle} text={w.kitText} />
          </Show>
        </div>
      </section>

      <Remember />
      <Leave />
    </>
  );
}

/** "Remember this browser" with a passkey where one can unlock it; else the plain line. */
function Remember() {
  const [can] = createResource(sync.passkeyAvailable);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const turnOn = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await sync.remember();
    } catch (e) {
      setError(say(e, "remember"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section class="set">
      <h2>{w.rememberTitle}</h2>
      <Show
        when={sync.remembered()}
        fallback={
          <Show when={can()} fallback={<p class="note small sync-lead">{can.loading ? "" : w.noPasskey}</p>}>
            <p class="note small sync-lead">{w.rememberText}</p>
            <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
            <p class="note small sync-lead">{w.notRemembered}</p>
            <div class="acts">
              <button class="btn" data-f="sync:remember" aria-busy={busy() || undefined} onClick={() => void turnOn()}>
                <Fingerprint aria-hidden />
                {busy() ? w.remembering : w.rememberButton}
              </button>
            </div>
          </Show>
        }
      >
        <Said tone="ok">{w.remembered}</Said>
        <div class="acts">
          <button class="btn" data-f="sync:forget-browser" onClick={() => void sync.forgetBrowser().catch(() => undefined)}>
            {w.forgetBrowser}
          </button>
        </div>
      </Show>
    </section>
  );
}

/** Codes after which the sync server will not let this device leave: only a local leave helps. */
const stuck = (code: string | undefined) => code === "fork" || code === "wrongRelay" || code === "relayBehind";

/** Stop syncing here only: nothing sent, this device forgets the account. */
function LeaveHere(props: { fkey: string }) {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const go = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await sync.signOut(true, true);
    } catch (e) {
      setError(say(e, "leave"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class="sync-leave-here">
      <p class="note small sync-lead">{w.leaveHereText}</p>
      <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
      <button class="btn danger" data-f={props.fkey} aria-busy={busy() || undefined} onClick={() => void go()}>
        <LogOut aria-hidden />
        {w.leaveHere}
      </button>
    </div>
  );
}

function Leave() {
  const [unsent, setUnsent] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [refused, setRefused] = createSignal(false);
  const leave = async (force: boolean) => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await sync.signOut(force);
    } catch (e) {
      if (codeOf(e) === "unsent") {
        setUnsent(true);
        focusSoon('[data-f="sync:leave-again"]');
      }
      if (stuck(codeOf(e))) setRefused(true);
      setError(say(e, "leave"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section class="set">
      <h2>{w.leaveTitle}</h2>
      <p class="note small sync-lead">{w.leaveText}</p>
      <Show when={sync.remembered()}>
        <p class="note small sync-lead">{w.passkeyAsk}</p>
      </Show>
      <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
      <div class="acts">
        <Show
          when={unsent()}
          fallback={
            <button class="btn" data-f="sync:leave" aria-busy={busy() || undefined} onClick={() => void leave(false)}>
              <LogOut aria-hidden />
              {w.leaveButton}
            </button>
          }
        >
          <button
            class="btn"
            data-f="sync:leave-again"
            onClick={() => {
              setUnsent(false);
              setError(null);
              void sync.syncNow();
            }}
          >
            <RefreshCw aria-hidden />
            {w.syncAgain}
          </button>
          <button class="btn danger" data-f="sync:leave-force" aria-busy={busy() || undefined} onClick={() => void leave(true)}>
            <LogOut aria-hidden />
            {w.leaveAnyway}
          </button>
        </Show>
      </div>
      <Show when={refused() || stuck(sync.problem()?.code)}>
        <LeaveHere fkey="sync:leave-here" />
      </Show>
    </section>
  );
}

/** A page that would make or join an account says so instead when this device already has one. */
function NoSecondAccount(props: { children: JSX.Element }) {
  return (
    <Show when={privateLink} fallback={<Said tone="bad">{w.plainLink}</Said>}>
      <Show
        when={sync.settled() && !sync.joined() && !sync.locked()}
        fallback={
          <Show when={sync.settled()} fallback={<Said tone="warn">{w.checking}</Said>}>
            <section class="problem">
              <p>{sync.locked() ? (sync.memoryProblem() ?? w.lockedFirst) : w.alreadyJoined}</p>
              <A class="btn primary" href="/sync" data-f="sync:back" data-autofocus>
                {w.title}
              </A>
            </section>
          </Show>
        }
      >
        {props.children}
      </Show>
    </Show>
  );
}

/** Start an account (the first device) or join one with a kit: both take the server and this device's name. */
function JoinForm(props: { kit: boolean }) {
  const navigate = useNavigate();
  const [server, setServer] = createSignal(sync.rememberedServer());
  const [name, setName] = createSignal(sync.defaultDeviceName());
  const [extra, setExtra] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  // The kit typed here leaves with the page.
  onCleanup(() => setExtra(""));
  const submit = async (e: Event) => {
    e.preventDefault();
    if (busy()) return;
    const s = server().trim();
    // An address with a user name or password is never kept or sent.
    if (!plainAddress(s)) return setError(w.badAddress);
    if (props.kit && !extra().trim()) return setError(w.enterKit);
    setBusy(true);
    setError(null);
    try {
      const n = name().trim() || "Marquee";
      if (props.kit) await sync.recover(s, extra().trim(), n);
      else await sync.createAccount(s, n, extra().trim());
      sync.rememberJoin(s, n);
      setExtra("");
      // A new account makes its kit next: without one, losing this device loses the account.
      navigate(props.kit || tv() ? "/sync" : "/sync/kit?first=1", { replace: true });
    } catch (err) {
      setError(say(err, props.kit ? "recover" : "start"));
      setBusy(false);
    }
  };
  return (
    <main class="browse page settings-page sync-page" data-vscroll>
      <h1>{props.kit ? w.recoverTitle : w.startTitle}</h1>
      <p class="lede">{props.kit ? w.recoverLede : w.startText}</p>
      <NoSecondAccount>
        <form class="install sync-form" onSubmit={(e) => void submit(e)}>
          <Field label={w.server} type="url" placeholder={w.serverHint} value={server()} onInput={setServer} fkey="sync-form:server" />
          <Show when={props.kit}>
            <Field label={w.kit} placeholder={w.kitHint} value={extra()} onInput={setExtra} fkey="sync-form:kit" mono secret />
          </Show>
          <Field label={w.device} value={name()} onInput={setName} fkey="sync-form:name" hint={w.deviceHint} />
          <Show when={!props.kit}>
            <Field label={w.invite} placeholder={w.inviteHint} value={extra()} onInput={setExtra} fkey="sync-form:invite" />
          </Show>
          <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
          <button class="btn primary" type="submit" data-f="sync-form:go" aria-busy={busy() || undefined} aria-disabled={busy() || undefined}>
            <Show when={busy()} fallback={props.kit ? <LifeBuoy aria-hidden /> : <Check aria-hidden />}>
              <i class="spin" aria-hidden="true" />
            </Show>
            {busy() ? (props.kit ? w.recovering : w.starting) : props.kit ? w.recoverButton : w.startButton}
          </button>
        </form>
        <p class="note small">{w.tabOnly}</p>
      </NoSecondAccount>
    </main>
  );
}

export const StartAccount = () => <JoinForm kit={false} />;
export const RecoverAccount = () => <JoinForm kit />;

/** What this browser's storage just did wrong, at the top of the page the user is on. */
function StorageNote() {
  return (
    <Show when={sync.memoryProblem()}>
      {(p) => (
        <div class="sync-note bad sync-storage" role="alert">
          <TriangleAlert aria-hidden />
          <span>{p()}</span>
          <Show when={p() === w.saveFailed}>
            <button class="btn" data-f="sync:save-again" onClick={() => void sync.saveAgain()}>
              {w.saveAgain}
            </button>
          </Show>
        </div>
      )}
    </Show>
  );
}

/** Pages that need a joined device say so instead of failing on their first op. */
function NeedsAccount(props: { children: JSX.Element }) {
  return (
    <Show
      when={sync.joined()}
      fallback={
        <section class="problem">
          <p>{sync.locked() ? w.locked : w.offLine}</p>
          <A class="btn primary" href="/sync" data-f="sync:back" data-autofocus>
            {w.title}
          </A>
        </section>
      }
    >
      <StorageNote />
      {props.children}
    </Show>
  );
}

type DetectorLike = { detect(source: HTMLVideoElement): Promise<{ rawValue: string }[]> };
type DetectorClass = new (o: { formats: string[] }) => DetectorLike;
const Detector = () => (window as unknown as { BarcodeDetector?: DetectorClass }).BarcodeDetector;
/** Camera scanning where the browser has both a QR reader and a camera API; the paste field is always there. */
const canScan = () => !!Detector() && !!navigator.mediaDevices?.getUserMedia;

function Scanner(props: { onCode: (code: string) => void; onClose: () => void }) {
  let video!: HTMLVideoElement;
  const [failed, setFailed] = createSignal(false);
  let stream: MediaStream | undefined;
  let stopped = false;
  const stop = () => {
    stopped = true;
    for (const t of stream?.getTracks() ?? []) t.stop();
  };
  onCleanup(stop);
  // A hidden tab never keeps the camera on.
  const hidden = () => document.hidden && props.onClose();
  document.addEventListener("visibilitychange", hidden);
  onCleanup(() => document.removeEventListener("visibilitychange", hidden));
  onMount(async () => {
    try {
      stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" } });
      if (stopped) return stop();
      video.srcObject = stream;
      await video.play();
      const detector = new (Detector() as DetectorClass)({ formats: ["qr_code"] });
      await watchForCode(
        async () => (await detector.detect(video)).map((c) => c.rawValue),
        () => stopped,
        (code) => {
          stop();
          props.onCode(code);
        },
      );
    } catch {
      stop();
      setFailed(true);
    }
  });
  return (
    <div class="sync-scan">
      <Show when={!failed()} fallback={<Said tone="bad">{w.noCamera}</Said>}>
        <video ref={video} muted playsinline aria-label={w.scanning} />
        <p class="step">{w.scanning}</p>
      </Show>
      <button class="btn" data-f="sync-add:scan-stop" data-autofocus onClick={props.onClose}>
        {w.cancel}
      </button>
    </div>
  );
}

export function AddDevice() {
  const navigate = useNavigate();
  const [scanning, setScanning] = createSignal(false);
  const [reviewing, setReviewing] = createSignal<string | null>(null);
  const [code, setCode] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [approved, setApproved] = createSignal<{ digits: string; code: string } | null>(null);
  const [differ, setDiffer] = createSignal<string | null>(null);
  const [differError, setDifferError] = createSignal<string | null>(null);
  const approve = async (qr: string) => {
    if (busy() || !qr.trim()) return;
    setScanning(false);
    setReviewing(null);
    setBusy(true);
    setError(null);
    try {
      setApproved({ digits: await sync.approvePairing(qr.trim()), code: qr.trim() });
      setCode("");
      focusSoon('[data-f="sync-add:done"]');
    } catch (e) {
      setError(say(e, "approve"));
    } finally {
      setBusy(false);
    }
  };
  // A scanned code is never approved on its own: the user reads where it is from first.
  const seen = (qr: string) => {
    setScanning(false);
    setReviewing(qr);
    focusSoon('[data-f="sync-add:review-cancel"]');
  };
  // The digits differ: whoever confirmed them is not the device in front of the user.
  const theyDiffer = async () => {
    const a = approved();
    if (!a || busy()) return;
    setBusy(true);
    setDifferError(null);
    try {
      // Ended through the core whether or not the device joined yet: the key rotates now.
      await sync.cancelPairing(a.code);
      setDiffer(w.differEnded);
      void sync.loadDevices().catch(() => undefined);
    } catch (e) {
      // A cancelled passkey or a failed call changes nothing: the button stays.
      setDifferError(say(e, "differ"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <main class="browse page settings-page sync-page" data-vscroll>
      <h1>{w.addTitle}</h1>
      <Show when={!tv()} fallback={<ElsewhereRow fkey="sync-add:tv" icon={<QrCode aria-hidden />} title={w.tvOnly} text={w.tvAdd} />}>
        <NeedsAccount>
          <Show
            when={approved()}
            fallback={
              <Show
                when={reviewing()}
                fallback={
                  <>
                    <p class="lede">{w.addLede}</p>
                    <p class="fa-note sync-callout">
                      <ShieldAlert aria-hidden />
                      {w.addWarning}
                    </p>
                    <Show
                      when={scanning()}
                      fallback={
                        <Show when={canScan()} fallback={<p class="note small sync-lead">{w.noScan}</p>}>
                          <button class="btn primary sync-scan-open" data-f="sync-add:scan" data-autofocus onClick={() => setScanning(true)}>
                            <Camera aria-hidden />
                            {w.scan}
                          </button>
                        </Show>
                      }
                    >
                      <Scanner onCode={seen} onClose={() => setScanning(false)} />
                    </Show>
                    <form
                      class="install sync-form"
                      onSubmit={(e) => {
                        e.preventDefault();
                        void approve(code());
                      }}
                    >
                      <Field label={canScan() ? w.pasteLabel : w.pasteOnly} placeholder={w.pasteHint} value={code()} onInput={setCode} fkey="sync-add:code" mono />
                      <button class="btn" classList={{ primary: !canScan() }} type="submit" data-f="sync-add:approve" aria-busy={busy() || undefined} aria-disabled={busy() || !code().trim() || undefined}>
                        <Show when={busy()} fallback={<Check aria-hidden />}>
                          <i class="spin" aria-hidden="true" />
                        </Show>
                        {busy() ? w.approving : w.approve}
                      </button>
                    </form>
                    <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
                  </>
                }
              >
                {(qr) => (
                  <section class="confirm" aria-labelledby="sync-review">
                    <h2 id="sync-review">{w.reviewAsk}</h2>
                    <p class="note">{w.reviewText}</p>
                    <div class="acts">
                      <button class="btn primary" data-f="sync-add:review-approve" onClick={() => void approve(qr())}>
                        <Check aria-hidden />
                        {w.approve}
                      </button>
                      <button class="btn" data-f="sync-add:review-cancel" data-autofocus onClick={() => setReviewing(null)}>
                        {w.cancel}
                      </button>
                    </div>
                  </section>
                )}
              </Show>
            }
          >
            {(a) => (
              <section class="confirm">
                <p class="step">{w.approvedStep}</p>
                <Spoken class="code" text={a().digits} label="Code" />
                <p class="note small">{w.approvedHelp}</p>
                <Show when={sync.remembered()}>
                  <p class="note small">{w.passkeyAsk}</p>
                </Show>
                <Show when={differ()}>{(d) => <Said tone="warn">{d()}</Said>}</Show>
                <Show when={differError()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
                <div class="acts">
                  <button class="btn primary" data-f="sync-add:done" data-autofocus onClick={() => navigate("/sync")}>
                    <Check aria-hidden />
                    {w.done}
                  </button>
                  <Show when={!differ()}>
                    <button class="btn" data-f="sync-add:differ" aria-busy={busy() || undefined} onClick={() => void theyDiffer()}>
                      <X aria-hidden />
                      {w.differ}
                    </button>
                  </Show>
                </div>
              </section>
            )}
          </Show>
        </NeedsAccount>
      </Show>
    </main>
  );
}

export function MakeKit() {
  const navigate = useNavigate();
  const [params] = useSearchParams<{ first?: string }>();
  // The kit lives in this signal only, for as long as the page is open.
  const [kit, setKit] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [copied, setCopied] = createSignal(false);
  onCleanup(() => {
    setKit(null);
    // A copied kit does not stay on the clipboard after the page.
    if (copied()) void navigator.clipboard.writeText("").catch(() => undefined);
  });
  const make = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      setKit(await sync.makeKit());
      focusSoon('[data-f="sync-kit:saved"]');
    } catch (e) {
      setError(say(e, "kit"));
    } finally {
      setBusy(false);
    }
  };
  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <main class="browse page settings-page sync-page" data-vscroll>
      <h1>{w.kitTitle}</h1>
      <Show when={!tv()} fallback={<ElsewhereRow fkey="sync-kit:tv" icon={<KeyRound aria-hidden />} title={w.tvOnly} text={w.tvKit} />}>
        <NeedsAccount>
          <Show
            when={kit()}
            fallback={
              <>
                <p class="lede">{params.first ? w.kitFirst : w.kitLede}</p>
                <p class="note small sync-lead">
                  {w.kitOnce}
                  <Show when={sync.remembered()}> {w.passkeyAsk}</Show>
                </p>
                <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
                <div class="acts">
                  <button class="btn primary" data-f="sync-kit:make" data-autofocus aria-busy={busy() || undefined} onClick={() => void make()}>
                    <Show when={busy()} fallback={<KeyRound aria-hidden />}>
                      <i class="spin" aria-hidden="true" />
                    </Show>
                    {busy() ? w.makingKit : w.makeKit}
                  </button>
                  <button class="btn" data-f="sync-kit:cancel" onClick={() => navigate("/sync", { replace: !!params.first })}>
                    {params.first ? w.later : w.cancel}
                  </button>
                </div>
              </>
            }
          >
            {(k) => (
              <section class="confirm sync-kit">
                <span class="kicker">{w.kitShown}</span>
                <Spoken class="sync-kit-text mono" text={k()} label={w.kitShown} />
                <p class="fa-note sync-callout">
                  <TriangleAlert aria-hidden />
                  {w.kitWarning}
                </p>
                <Show when={copied()}>
                  <Said tone="warn">{w.kitClipboard}</Said>
                </Show>
                <div class="acts">
                  <button class="btn" data-f="sync-kit:copy" onClick={() => void copy(k())}>
                    <Show when={copied()} fallback={<Copy aria-hidden />}>
                      <Check aria-hidden />
                    </Show>
                    {copied() ? w.copied : w.copyKit}
                  </button>
                  <button class="btn primary" data-f="sync-kit:saved" data-autofocus onClick={() => navigate("/sync", { replace: true })}>
                    <Check aria-hidden />
                    {w.savedKit}
                  </button>
                </div>
              </section>
            )}
          </Show>
        </NeedsAccount>
      </Show>
    </main>
  );
}

export function ReportLost() {
  const params = useParams<{ id: string }>();
  const navigate = useNavigate();
  return (
    <main class="browse page settings-page sync-page" data-vscroll>
      <NeedsAccount>
        <Lost id={params.id} done={() => navigate("/sync", { replace: true })} />
      </NeedsAccount>
    </main>
  );
}

function Lost(props: { id: string; done: () => void }) {
  // Always read again: a device added since the list was shown must be on this page.
  const [data, { refetch }] = createResource(async () => {
    const list = await sync.loadDevices();
    return { members: list.members, preview: await sync.lostPreview(props.id) };
  });
  // A failed resource throws when read: nothing to show until it loads again.
  const got = () => (data.error ? undefined : data());
  const members = () => got()?.members ?? [];
  // The name may come from the list already on screen while the new one loads.
  const lost = () => (got()?.members ?? sync.devices()?.members ?? []).find((m) => m.id === props.id);
  const name = () => (lost() ? memberName(lost() as MemberInfo) : w.unknownDevice);
  const rows = () => lostRows(got()?.preview.devices ?? [], members());
  const [keep, setKeep] = createSignal<ReadonlySet<string>>(new Set());
  const toggle = (id: string) => {
    const next = new Set(keep());
    if (!next.delete(id)) next.add(id);
    setKeep(next);
  };
  const drop = createMemo(() => lostDrop(props.id, got()?.preview.devices ?? [], keep()));
  const kits = createMemo(() => kitsGoing(got()?.preview.kits ?? [], members(), drop()));
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const report = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await sync.reportLost(props.id, [...keep()]);
    } catch (e) {
      setError(say(e, "lost"));
      setBusy(false);
      return;
    }
    props.done();
    void sync.loadDevices().catch(() => undefined);
  };
  // Focus starts on the first row to untick, else on Cancel: never on the button that removes.
  createEffect(on(() => data.state, (s) => s === "ready" && focusSoon(rows().length ? ".sync-check" : '[data-f="sync-lost:cancel"]')));
  return (
    <>
      <h1>{`${w.reportLost}: ${name()}`}</h1>
      <p class="lede">{named(w.lostLede, name())}</p>
      <Show
        when={!data.error}
        fallback={
          <section class="problem">
            <Said tone="bad">{say(data.error, "load")}</Said>
            <div class="acts">
              <button class="btn primary" data-f="sync-lost:retry" data-autofocus onClick={() => void refetch()}>
                <RefreshCw aria-hidden />
                {w.retry}
              </button>
              <button class="btn" data-f="sync-lost:cancel" onClick={props.done}>
                {w.cancel}
              </button>
            </div>
          </section>
        }
      >
        <Show when={got()} fallback={<Said tone="warn">{named(w.lostChecking, name())}</Said>}>
          <Show when={rows().length}>
            <section class="set">
              <h2>{named(w.lostAdded, name())}</h2>
              <p class="note small sync-lead">{named(w.lostAddedHelp, name())}</p>
              <ul class="sync-members" role="group" aria-label={named(w.lostAdded, name())}>
                <For each={rows()}>
                  {(row) => (
                    <li>
                      <button class="sync-member sync-check" role="checkbox" aria-checked={!keep().has(row.id)} data-f={`sync-lost:${row.id}`} onClick={() => toggle(row.id)}>
                        <span class="sync-box" aria-hidden="true">
                          <Show when={!keep().has(row.id)}>
                            <Check />
                          </Show>
                        </span>
                        <span class="sync-mtext">
                          <b dir="auto">{row.member ? memberName(row.member) : w.unknownDevice}</b>
                          <small>{row.member ? addedLine(false, row.member.addedMs, sponsorName(row.member, members())) : w.unknownLine}</small>
                        </span>
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </section>
          </Show>
          <Show when={kits().length}>
            <p class="fa-note sync-callout">
              <KeyRound aria-hidden />
              {lostKits(kits().length)}
            </p>
          </Show>
          <Show when={sync.remembered()}>
            <p class="note small sync-lead">{w.passkeyAsk}</p>
          </Show>
          <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
          <div class="acts sync-lost-acts">
            <button class="btn danger" data-f="sync-lost:go" aria-busy={busy() || undefined} onClick={() => void report()}>
              <ShieldAlert aria-hidden />
              {lostButton(drop().length)}
            </button>
            <button class="btn" data-f="sync-lost:cancel" onClick={props.done}>
              {w.cancel}
            </button>
          </div>
        </Show>
      </Show>
    </>
  );
}

/** After the sync server showed a different device list: both lists, the
 * user's ticks, then the server's list, never before the user saw it. */
export function ForkReview() {
  const navigate = useNavigate();
  const done = () => navigate("/sync", { replace: true });
  // Only a device whose list differs compares: anything else would rotate the key for nothing.
  // Read once: continuing clears the fork before the page leaves, which must not flip it.
  const fork = untrack(() => sync.problem()?.code === "fork");
  const forked = () => fork;
  const [data, { refetch }] = createResource(() => forked() || undefined, () => sync.forkPreview());
  const got = () => (data.error ? undefined : data());
  const sides = () => {
    const d = got();
    return d ? forkSides(d.mine, d.server) : null;
  };
  const server = () => sortMembers(got()?.server ?? []);
  const all = () => [...(got()?.server ?? []), ...(got()?.mine ?? [])];
  const goes = (id: string) => got()?.drops.includes(id) === true;
  const [ticked, setTicked] = createSignal<ReadonlySet<string>>(new Set());
  const toggle = (id: string) => {
    const next = new Set(ticked());
    if (!next.delete(id)) next.add(id);
    setTicked(next);
  };
  const drop = () => forkDrop(got()?.server ?? [], got()?.drops ?? [], ticked());
  const kitsOf = (id: string) => got()?.kits.find((k) => k.device === id)?.kits.filter((k) => !goes(k)) ?? [];
  // A kit whose maker is ticked goes with it: shown as going, not as a box of its own.
  const withTicked = (m: MemberInfo) => m.kit && !!m.sponsor && ticked().has(m.sponsor);
  const isKit = (id: string) => got()?.server.some((m) => m.id === id && m.kit) === true;
  const kitsGo = () => forkKits(got()?.kits ?? [], drop(), got()?.drops ?? []);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const go = async () => {
    const d = got();
    if (busy() || !d) return;
    setBusy(true);
    setError(null);
    try {
      // Only the list the user saw: one that moved since is refused, then shown again.
      await sync.useServerRoster(drop(), d.head);
    } catch (e) {
      setError(say(e, "serverList"));
      setBusy(false);
      if (codeOf(e) === "rosterChanged") {
        setTicked(new Set<string>());
        void refetch();
      }
      return;
    }
    done();
  };
  createEffect(on(() => data.state, (s) => s === "ready" && focusSoon('[data-f="sync-fork:cancel"]')));
  const tag = (m: MemberInfo) => (sides()?.serverOnly.some((x) => x.id === m.id) ? " (only on the server's list)" : "");
  return (
    <main class="browse page settings-page sync-page" data-vscroll>
      <h1>{w.forkTitle}</h1>
      <p class="lede">{w.forkLede}</p>
      <NeedsAccount>
        <Show
          when={forked()}
          fallback={
            <section class="problem">
              <p>{w.forkNone}</p>
              <A class="btn primary" href="/sync" data-f="sync-fork:back" data-autofocus>
                {w.title}
              </A>
            </section>
          }
        >
        <Show
          when={!data.error}
          fallback={
            <section class="problem">
              <Said tone="bad">{say(data.error, "compare")}</Said>
              <Show when={codeOf(data.error) === undefined && String((data.error as Error | undefined)?.message) === "http status 401"}>
                <LeaveHere fkey="sync-fork:leave-here" />
              </Show>
              <div class="acts">
                <button class="btn primary" data-f="sync-fork:retry" data-autofocus onClick={() => void refetch()}>
                  <RefreshCw aria-hidden />
                  {w.retry}
                </button>
                <button class="btn" data-f="sync-fork:cancel" onClick={done}>
                  {w.cancel}
                </button>
              </div>
            </section>
          }
        >
          <Show when={got()} fallback={<Said tone="warn">{w.forkLoading}</Said>}>
          <Show
            when={got()?.onServer}
            fallback={
              <section class="problem">
                <Said tone="bad">{w.forkOffList}</Said>
                <div class="acts">
                  <button class="btn" data-f="sync-fork:cancel" data-autofocus onClick={done}>
                    {w.cancel}
                  </button>
                </div>
                <LeaveHere fkey="sync-fork:leave-here" />
              </section>
            }
          >
            <section class="set">
              <h2>{w.forkServer}</h2>
              <p class="note small sync-lead">{w.forkServerHelp}</p>
              <ul class="sync-members" role="group" aria-label={w.forkServer}>
                <For each={server()}>
                  {(m) => (
                    <li>
                      <Show
                        when={!m.thisDevice && !goes(m.id) && !withTicked(m)}
                        fallback={
                          <div class="sync-member" classList={{ me: m.thisDevice }} data-member={m.id}>
                            <span class="sync-mtext">
                              <b dir="auto">
                                {memberName(m)}
                                {tag(m)}
                                <span class="sync-tag" classList={{ bad: goes(m.id) }}>
                                  {m.thisDevice ? w.thisDevice : goes(m.id) ? w.forkGoes : named(w.goesWithTicked, sponsorName(m, all()) ?? "")}
                                </span>
                              </b>
                              <small>{addedLine(m.kit, m.addedMs, sponsorName(m, all()))}</small>
                            </span>
                          </div>
                        }
                      >
                        <button class="sync-member sync-check" role="checkbox" aria-checked={ticked().has(m.id)} data-f={`sync-fork:${m.id}`} data-member={m.id} onClick={() => toggle(m.id)}>
                          <span class="sync-box" aria-hidden="true">
                            <Show when={ticked().has(m.id)}>
                              <Check />
                            </Show>
                          </span>
                          <span class="sync-mtext">
                            <b dir="auto">
                              {memberName(m)}
                              {tag(m)}
                            </b>
                            <small>{addedLine(m.kit, m.addedMs, sponsorName(m, all()))}</small>
                            <Show when={ticked().has(m.id) && kitsOf(m.id).length}>
                              <small class="sync-goes">{goesWith(kitsOf(m.id).length)}</small>
                            </Show>
                          </span>
                        </button>
                      </Show>
                    </li>
                  )}
                </For>
              </ul>
            </section>
            <Show when={sides()?.mineOnly.length} fallback={<Show when={!sides()?.serverOnly.length}>{<p class="note small">{w.forkSame}</p>}</Show>}>
              <section class="set">
                <h2>{w.forkMine}</h2>
                <p class="note small sync-lead">{w.forkMineHelp}</p>
                <ul class="sync-members">
                  <For each={sortMembers(sides()?.mineOnly ?? [])}>
                    {(m) => (
                      <li class="sync-member" data-member={m.id}>
                        <span class="sync-mtext">
                          <b dir="auto">{memberName(m)}</b>
                          <small>{addedLine(m.kit, m.addedMs, sponsorName(m, all()))}</small>
                        </span>
                      </li>
                    )}
                  </For>
                </ul>
              </section>
            </Show>
            <Show when={sync.remembered()}>
              <p class="note small sync-lead">{w.passkeyAsk}</p>
            </Show>
            <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
            <div class="acts sync-lost-acts">
              <button class="btn" classList={{ danger: drop().length > 0 }} data-f="sync-fork:go" aria-busy={busy() || undefined} onClick={() => void go()}>
                <MonitorSmartphone aria-hidden />
                {busy() ? w.forkGoing : forkButton(drop().filter((id) => !isKit(id)).length, new Set([...drop().filter(isKit), ...kitsGo()]).size)}
              </button>
              <button class="btn" data-f="sync-fork:cancel" onClick={done}>
                {w.cancel}
              </button>
            </div>
          </Show>
          </Show>
        </Show>
        </Show>
      </NeedsAccount>
    </main>
  );
}
