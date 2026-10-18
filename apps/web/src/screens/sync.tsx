/** Devices and sync (plans/m7-sync.md, plans/core-protocol.md): start an
 * account, join one with a recovery kit, see this device's state and the
 * others, add a device, make a kit, remove a device or report one lost, stop
 * syncing. The account's record lives in this tab only (src/core/sync.ts);
 * a kit is shown once and kept nowhere. No key, token or kit reaches a log. */

import { A, useNavigate, useParams } from "@solidjs/router";
import Camera from "lucide-solid/icons/camera";
import Check from "lucide-solid/icons/check";
import Copy from "lucide-solid/icons/copy";
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
import { createEffect, createMemo, createResource, createSignal, For, type JSX, on, onCleanup, onMount, Show } from "solid-js";
import { BridgeError } from "../core/bridge";
import type { MemberInfo } from "../core/generated/MemberInfo";
import * as sync from "../core/sync";
import { autoFocus } from "../focus/dom";
import { form } from "../form";
import { fingerprintGroups, isPairingCode, kitsGoing, lostDrop, memberName, sortMembers, sponsorName } from "../lib/sync";
import { addedLine, joinProblem, lostButton, lostKits, named, plainAddress, removeDeviceAsk, reportNotes, syncWords as w } from "../lib/words";
import { LinkRow } from "./settings";
import "./browse.css";
import "./settings.css";
import "./sync.css";

const tv = () => form() === "tv";
const errorParts = (e: unknown): [string | undefined, string] => [e instanceof BridgeError ? e.code : undefined, e instanceof Error ? e.message : String(e)];
const clock = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
/** After the page changed under it, focus goes to the step's own target (remote and keys only). */
const focusSoon = (sel: string) => queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>(sel)));

function Said(props: { tone: "ok" | "warn" | "bad" | ""; children: JSX.Element }) {
  return (
    <p class={`st ${props.tone}`} role="status">
      <i class="dot" />
      {props.children}
    </p>
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

function Field(props: { label: string; value: string; onInput: (v: string) => void; fkey: string; type?: string; placeholder?: string; mono?: boolean; hint?: string }) {
  return (
    <label class="field-line">
      <span class="kicker">{props.label}</span>
      <input
        type={props.type ?? "text"}
        classList={{ mono: props.mono }}
        placeholder={props.placeholder}
        value={props.value}
        onInput={(e) => props.onInput(e.currentTarget.value)}
        data-f={props.fkey}
        autocomplete="off"
        autocapitalize="off"
        spellcheck={false}
      />
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
      <Show when={sync.joined()} fallback={<NotJoined />}>
        <Joined />
      </Show>
    </main>
  );
}

function NotJoined() {
  return (
    <>
      <Show when={sync.problem()}>{(p) => <Said tone="bad">{p().words}</Said>}</Show>
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
      (e) => setListError(joinProblem(...errorParts(e), "approve")),
    );
  onMount(() => void load());
  const now = async () => {
    await sync.syncNow();
    if (sync.joined()) void load();
  };
  const notes = () => {
    const r = sync.report();
    return r ? reportNotes(r) : [];
  };
  const fingerprint = () => sync.report()?.fingerprint || sync.devices()?.fingerprint;
  const state = (): { tone: "ok" | "warn" | "bad" | ""; text: string } => {
    const p = sync.problem();
    if (p) return { tone: "bad", text: p.words };
    if (sync.syncing()) return { tone: "warn", text: w.syncing };
    const at = sync.syncedAt();
    return at ? { tone: "ok", text: `Synced at ${clock(at)}` } : { tone: "", text: w.notSynced };
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
      setAsking(null);
      setSaid({ id: "list", tone: "ok", text: `${w.removed}: ${memberName(m)}` });
      focusSoon('[data-f="sync:now"]');
    } catch (e) {
      setSaid({ id: m.id, tone: "bad", text: joinProblem(...errorParts(e), "approve") });
    } finally {
      setBusy(false);
    }
  };
  const ask = (id: string | null) => {
    setAsking(id);
    setSaid(null);
    focusSoon(id ? `[data-f="sync:${id}:cancel"]` : '[data-f="sync:now"]');
  };
  return (
    <>
      <section class="set sync-card" aria-label={w.thisDevice}>
        <div class="sync-state">
          <Said tone={state().tone}>{state().text}</Said>
          <For each={notes()}>
            {(n) => (
              <p class="sync-note">
                <TriangleAlert aria-hidden />
                {n}
              </p>
            )}
          </For>
        </div>
        <div class="acts">
          <button class="btn primary" data-f="sync:now" data-autofocus aria-busy={sync.syncing() || undefined} onClick={() => void now()}>
            <RefreshCw aria-hidden classList={{ spin: sync.syncing() }} />
            {sync.syncing() ? w.syncing : w.syncNow}
          </button>
          <Show when={sync.problem()?.code === "fork"}>
            <button class="btn" data-f="sync:server-list" onClick={() => void sync.useServerRoster().then(load, (e) => setSaid({ id: "list", tone: "bad", text: joinProblem(...errorParts(e), "approve") }))}>
              <MonitorSmartphone aria-hidden />
              {w.useServer}
            </button>
          </Show>
        </div>
        <Show when={fingerprint()}>
          {(fp) => (
            <div class="sync-compare">
              <span class="kicker">{w.compareLabel}</span>
              <p class="sync-fp mono" aria-label={`${w.compareLabel}: ${[...fp()].join(" ")}`}>
                {fingerprintGroups(fp())}
              </p>
              <p class="note small">{w.compareHelp}</p>
            </div>
          )}
        </Show>
      </section>

      <Show when={sync.recovered() || noKit()}>
        <p class="fa-note sync-callout">
          <KeyRound aria-hidden />
          {sync.recovered() ? w.afterRecover : w.noKit}
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
                <li class="sync-member" classList={{ me: m.thisDevice }}>
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

      <Leave />
      <p class="note small">{w.tabOnly}</p>
    </>
  );
}

function Leave() {
  const [unsent, setUnsent] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const leave = async (force: boolean) => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await sync.signOut(force);
    } catch (e) {
      const [code, message] = errorParts(e);
      if (code === "unsent") {
        setUnsent(true);
        focusSoon('[data-f="sync:leave-again"]');
      }
      setError(joinProblem(code, message, "approve"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section class="set">
      <h2>{w.leaveTitle}</h2>
      <p class="note small sync-lead">{w.leaveText}</p>
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
    </section>
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
  const submit = async (e: Event) => {
    e.preventDefault();
    if (busy()) return;
    const s = server().trim();
    // An address with a user name or password is never kept or sent.
    if (!plainAddress(s)) return setError(w.badAddress);
    if (props.kit && !extra().trim()) return setError("Enter the recovery kit.");
    setBusy(true);
    setError(null);
    try {
      const n = name().trim() || "Marquee";
      if (props.kit) await sync.recover(s, extra().trim(), n);
      else await sync.createAccount(s, n, extra().trim());
      rememberAndGo(s, n);
    } catch (err) {
      setError(joinProblem(...errorParts(err), props.kit ? "recover" : "start"));
      setBusy(false);
    }
  };
  const rememberAndGo = (s: string, n: string) => {
    sync.remember(s, n);
    // The kit leaves the page with it: nothing keeps it.
    setExtra("");
    navigate("/sync", { replace: true });
  };
  return (
    <main class="browse page settings-page sync-page" data-vscroll>
      <h1>{props.kit ? w.recoverTitle : w.startTitle}</h1>
      <p class="lede">{props.kit ? "Joins your account as a new device. The kit is the 32 letters and digits you saved when you made it." : w.startText}</p>
      <form class="install sync-form" onSubmit={(e) => void submit(e)}>
        <Field label={w.server} type="url" placeholder={w.serverHint} value={server()} onInput={setServer} fkey="sync-form:server" />
        <Show when={props.kit}>
          <Field label={w.kit} placeholder={w.kitHint} value={extra()} onInput={setExtra} fkey="sync-form:kit" mono />
        </Show>
        <Field label={w.device} value={name()} onInput={setName} fkey="sync-form:name" hint="Your other devices list it by this name." />
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
    </main>
  );
}

export const StartAccount = () => <JoinForm kit={false} />;
export const RecoverAccount = () => <JoinForm kit />;

/** Pages that need a joined device say so instead of failing on their first op. */
function NeedsAccount(props: { children: JSX.Element }) {
  return (
    <Show
      when={sync.joined()}
      fallback={
        <section class="problem">
          <p>{w.offLine}</p>
          <A class="btn primary" href="/sync" data-f="sync:back" data-autofocus>
            {w.title}
          </A>
        </section>
      }
    >
      {props.children}
    </Show>
  );
}

type BarcodeDetectorLike = { detect(source: HTMLVideoElement): Promise<{ rawValue: string }[]> };
type DetectorClass = new (o: { formats: string[] }) => BarcodeDetectorLike;
const Detector = () => (window as unknown as { BarcodeDetector?: DetectorClass }).BarcodeDetector;
/** Camera scanning where the browser has both a QR reader and a camera API; the paste field is always there. */
const canScan = () => !!Detector() && !!navigator.mediaDevices?.getUserMedia;

function Scanner(props: { onCode: (code: string) => void; onClose: () => void }) {
  let video!: HTMLVideoElement;
  const [failed, setFailed] = createSignal(false);
  let stream: MediaStream | undefined;
  let timer: number | undefined;
  let stopped = false;
  const stop = () => {
    stopped = true;
    clearTimeout(timer);
    for (const t of stream?.getTracks() ?? []) t.stop();
  };
  onCleanup(stop);
  onMount(async () => {
    try {
      stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" } });
      if (stopped) return stop();
      video.srcObject = stream;
      await video.play();
      const detector = new (Detector() as DetectorClass)({ formats: ["qr_code"] });
      const look = async () => {
        if (stopped) return;
        const found = await detector.detect(video).catch(() => []);
        const code = found.find((c) => isPairingCode(c.rawValue));
        if (code) {
          stop();
          props.onCode(code.rawValue.trim());
        } else timer = window.setTimeout(() => void look(), 250);
      };
      void look();
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
  const [code, setCode] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [digits, setDigits] = createSignal<string | null>(null);
  const approve = async (qr: string) => {
    if (busy() || !qr.trim()) return;
    setScanning(false);
    setBusy(true);
    setError(null);
    try {
      setDigits(await sync.approvePairing(qr.trim()));
      setCode("");
      focusSoon('[data-f="sync-add:done"]');
    } catch (e) {
      setError(joinProblem(...errorParts(e), "approve"));
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
            when={digits()}
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
                    <Show when={canScan()}>
                      <button class="btn primary sync-scan-open" data-f="sync-add:scan" data-autofocus onClick={() => setScanning(true)}>
                        <Camera aria-hidden />
                        {w.scan}
                      </button>
                    </Show>
                  }
                >
                  <Scanner onCode={(c) => void approve(c)} onClose={() => setScanning(false)} />
                </Show>
                <form
                  class="install sync-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void approve(code());
                  }}
                >
                  <Field label={w.pasteLabel} placeholder={w.pasteHint} value={code()} onInput={setCode} fkey="sync-add:code" mono />
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
            {(d) => (
              <section class="confirm">
                <p class="step">{w.approvedStep}</p>
                <p class="code" aria-label={`Code ${d().split("").join(" ")}`}>
                  {d()}
                </p>
                <p class="note small">{w.approvedHelp}</p>
                <div class="acts">
                  <button class="btn primary" data-f="sync-add:done" data-autofocus onClick={() => navigate("/sync")}>
                    <Check aria-hidden />
                    {w.done}
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

export function MakeKit() {
  const navigate = useNavigate();
  // The kit lives in this signal only, for as long as the page is open.
  const [kit, setKit] = createSignal<string | null>(null);
  onCleanup(() => setKit(null));
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [copied, setCopied] = createSignal(false);
  const make = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      setKit(await sync.makeKit());
      focusSoon('[data-f="sync-kit:saved"]');
    } catch (e) {
      setError(joinProblem(...errorParts(e), "approve"));
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
                <p class="lede">{w.kitLede}</p>
                <p class="note small sync-lead">{w.kitOnce}</p>
                <Show when={error()}>{(e) => <Said tone="bad">{e()}</Said>}</Show>
                <div class="acts">
                  <button class="btn primary" data-f="sync-kit:make" data-autofocus aria-busy={busy() || undefined} onClick={() => void make()}>
                    <Show when={busy()} fallback={<KeyRound aria-hidden />}>
                      <i class="spin" aria-hidden="true" />
                    </Show>
                    {busy() ? w.makingKit : w.makeKit}
                  </button>
                  <button class="btn" data-f="sync-kit:cancel" onClick={() => navigate("/sync")}>
                    {w.cancel}
                  </button>
                </div>
              </>
            }
          >
            {(k) => (
              <section class="confirm sync-kit">
                <span class="kicker">{w.kitShown}</span>
                <p class="sync-kit-text mono" aria-label={`${w.kitShown}: ${k().split("").join(" ")}`}>
                  {k()}
                </p>
                <p class="fa-note sync-callout">
                  <TriangleAlert aria-hidden />
                  {w.kitWarning}
                </p>
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
  // Names and dates come from the device list; the preview says what goes.
  const [data] = createResource(async () => {
    const list = sync.devices() ?? (await sync.loadDevices());
    return { members: list.members, preview: await sync.lostPreview(props.id) };
  });
  const members = () => data()?.members ?? sync.devices()?.members ?? [];
  const lost = () => members().find((m) => m.id === props.id);
  const name = () => (lost() ? memberName(lost() as MemberInfo) : w.unnamed);
  const added = () => sortMembers(members().filter((m) => data()?.preview.devices.includes(m.id)));
  const [keep, setKeep] = createSignal<ReadonlySet<string>>(new Set());
  const toggle = (id: string) => {
    const next = new Set(keep());
    if (!next.delete(id)) next.add(id);
    setKeep(next);
  };
  const drop = createMemo(() => lostDrop(props.id, data()?.preview.devices ?? [], keep()));
  const kits = createMemo(() => kitsGoing(data()?.preview.kits ?? [], members(), drop()));
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const report = async () => {
    if (busy()) return;
    setBusy(true);
    setError(null);
    try {
      await sync.reportLost(props.id, [...keep()]);
      props.done();
    } catch (e) {
      setError(joinProblem(...errorParts(e), "approve"));
      setBusy(false);
    }
  };
  createEffect(on(() => data.state, (s) => s === "ready" && focusSoon(added().length ? ".sync-check" : '[data-f="sync-lost:cancel"]')));
  return (
    <>
      <h1>{`${w.reportLost}: ${name()}`}</h1>
      <p class="lede">{named(w.lostLede, name())}</p>
      <Show when={!data.error} fallback={<Said tone="bad">{joinProblem(...errorParts(data.error), "approve")}</Said>}>
        <Show when={data()} fallback={<Said tone="warn">{named(w.lostChecking, name())}</Said>}>
          <Show when={added().length}>
            <section class="set">
              <h2>{named(w.lostAdded, name())}</h2>
              <p class="note small sync-lead">{named(w.lostAddedHelp, name())}</p>
              <ul class="sync-members" role="group" aria-label={named(w.lostAdded, name())}>
                <For each={added()}>
                  {(m) => (
                    <li>
                      <button class="sync-member sync-check" role="checkbox" aria-checked={!keep().has(m.id)} data-f={`sync-lost:${m.id}`} onClick={() => toggle(m.id)}>
                        <span class="sync-box" aria-hidden="true">
                          <Show when={!keep().has(m.id)}>
                            <Check />
                          </Show>
                        </span>
                        <span class="sync-mtext">
                          <b dir="auto">{memberName(m)}</b>
                          <small>{addedLine(false, m.addedMs, sponsorName(m, members()))}</small>
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
