/** Debrid services (plans/c10-handoff.md, phase 3a): each account is a record
 * made by a sign-in with a code or a typed key. A browser keeps records only in
 * the tab; the core keeps the accounts while it runs. No key, token or code
 * reaches a log. */

import Check from "lucide-solid/icons/check";
import ExternalLink from "lucide-solid/icons/external-link";
import KeyRound from "lucide-solid/icons/key-round";
import QrCode from "lucide-solid/icons/qr-code";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Trash from "lucide-solid/icons/trash";
import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { Qr } from "../components/qr";
import { liveView, op, reasonOf } from "../core/client";
import { debridAccounts, debridStore } from "../core/debrid";
import type { DebridAccount } from "../core/generated/DebridAccount";
import type { DebridLoginSnapshot } from "../core/generated/DebridLoginSnapshot";
import { autoFocus } from "../focus/dom";
import { form } from "../form";
import { address, CODE_SERVICES, type DebridService, loginWords, SERVICES, stateWords } from "../lib/debrid";
import { waitText } from "../lib/profiles";
import "./browse.css";
import "./settings.css";

type Said = { service: string; text: string; tone: "ok" | "bad" | "idle" };

export default function Debrid() {
  const [signingIn, setSigningIn] = createSignal<string | null>(null);
  const [said, setSaid] = createSignal<Said | null>(null);
  const account = (id: string) => debridAccounts()?.accounts.find((a) => a.service === id);
  const pending = (id: string) => debridAccounts()?.pending.some((r) => r.service === id) === true;
  return (
    <main class="browse page settings-page debrid-page" data-vscroll>
      <h1>Debrid services</h1>
      <p class="lede">A debrid account plays torrents as fast direct links. Sign-ins and keys are never shown again or synced.</p>
      <p class="note small">A browser cannot store them safely: they last while Marquee's core runs. The apps keep them encrypted on the device.</p>
      <div class="dcards">
        <For each={SERVICES}>
          {(s) => (
            <Card
              service={s}
              account={account(s.id)}
              pending={pending(s.id)}
              signingIn={signingIn() === s.id}
              said={said()?.service === s.id ? said() : null}
              onSignIn={() => {
                setSaid(null);
                setSigningIn(s.id);
              }}
              onClose={(text) => {
                setSigningIn(null);
                if (text) setSaid({ service: s.id, text, tone: "ok" });
                // Focus goes back to the card that opened it: its sign-in, else its Disconnect.
                queueMicrotask(() =>
                  autoFocus(document.querySelector<HTMLElement>(`[data-f="debrid:${s.id}:code"], [data-f="debrid:${s.id}:disconnect"]`)),
                );
              }}
              onSaid={setSaid}
            />
          )}
        </For>
      </div>
    </main>
  );
}

function Card(props: {
  service: DebridService;
  account: DebridAccount | undefined;
  pending: boolean;
  signingIn: boolean;
  said: Said | null;
  onSignIn: () => void;
  onClose: (said?: string) => void;
  onSaid: (said: Said | null) => void;
}) {
  const id = () => props.service.id;
  const byCode = () => CODE_SERVICES.has(id());
  const [keyOpen, setKeyOpen] = createSignal(false);
  const [draft, setDraft] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const needsSignIn = () => !props.account || props.account.state === "reconnect";
  // Premiumize has only a key; the others open it with "Use a key instead".
  const keyField = () => !props.signingIn && (keyOpen() || (!byCode() && needsSignIn()));
  const state = () => stateWords(props.account?.state, props.pending);
  const tone = () => (props.account?.state === "reconnect" ? "bad" : props.account?.state === "failing" || props.pending ? "warn" : "ok");

  const save = async (e: Event) => {
    e.preventDefault();
    const key = draft().trim();
    if (!key || busy()) return;
    setBusy(true);
    setDraft("");
    try {
      await op("setDebridKey", { service: id(), key });
      setKeyOpen(false);
      props.onSaid(null);
    } catch (err) {
      props.onSaid({ service: id(), text: `The key was not taken: ${reasonOf(err)}.`, tone: "bad" });
    } finally {
      setBusy(false);
    }
  };
  const disconnect = async () => {
    const a = props.account;
    if (!a || busy()) return;
    setBusy(true);
    try {
      await debridStore.disconnect(a.service, a.generation);
      props.onSaid({ service: id(), text: "Disconnected", tone: "idle" });
    } catch (err) {
      props.onSaid({ service: id(), text: `Not disconnected yet: ${reasonOf(err)}. It finishes at the next start.`, tone: "bad" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section class="dcard" aria-label={props.service.name}>
      <div class="dcard-head">
        <h2>{props.service.name}</h2>
        <Show when={state()}>
          {(words) => (
            <p class={`st ${tone()}`} role="status">
              <i class="dot" />
              {words()}
            </p>
          )}
        </Show>
      </div>
      <Show
        when={!props.signingIn}
        fallback={<SignIn service={props.service} account={props.account} pending={props.pending} onClose={props.onClose} />}
      >
        <div class="dcard-acts">
          <Show when={byCode() && needsSignIn()}>
            <button class="btn primary" data-f={`debrid:${id()}:code`} aria-label={`Sign in to ${props.service.name} with a code`} onClick={props.onSignIn}>
              <QrCode aria-hidden />
              {props.account ? "Sign in again" : "Sign in with a code"}
            </button>
            <Show when={!keyOpen()}>
              <button class="btn" data-f={`debrid:${id()}:key`} aria-label={`Use a key for ${props.service.name} instead`} onClick={() => setKeyOpen(true)}>
                <KeyRound aria-hidden />
                Use a key instead
              </button>
            </Show>
          </Show>
          <Show when={props.account}>
            {/* A connected account can take a new key without being disconnected first. */}
            <Show when={!keyOpen() && props.account?.state !== "reconnect"}>
              <button class="btn" data-f={`debrid:${id()}:key`} aria-label={`Use a new key for ${props.service.name}`} onClick={() => setKeyOpen(true)}>
                <KeyRound aria-hidden />
                {byCode() ? "Use a key instead" : "Replace the key"}
              </button>
            </Show>
            <button class="btn" data-f={`debrid:${id()}:disconnect`} aria-label={`Disconnect ${props.service.name}`} aria-disabled={busy()} onClick={() => void disconnect()}>
              <Trash aria-hidden />
              Disconnect
            </button>
          </Show>
        </div>
      </Show>
      <Show when={keyField()}>
        <form class="dkey" onSubmit={(e) => void save(e)}>
          <label class="field-line">
            <span class="kicker">{props.service.name} API key</span>
            <input
              type="password"
              value={draft()}
              onInput={(e) => setDraft(e.currentTarget.value)}
              placeholder="API key"
              autocomplete="off"
              spellcheck={false}
              data-f={`debrid:${id()}:field`}
            />
          </label>
          <button class="btn primary" type="submit" data-f={`debrid:${id()}:save`} aria-label={`Save ${props.service.name} key`} aria-disabled={!draft().trim() || busy()}>
            <Check aria-hidden />
            Save
          </button>
          <a class="dlink" href={props.service.keyPage} target="_blank" rel="noreferrer" data-f={`debrid:${id()}:page`}>
            Get a key at {address(props.service.keyPage)}
          </a>
        </form>
      </Show>
      <Show when={props.said}>
        {(m) => (
          <p class={`st ${m().tone === "idle" ? "" : m().tone}`} role="status">
            <i class="dot" />
            {m().text}
          </p>
        )}
      </Show>
    </section>
  );
}

/** A sign-in by code: the QR and the code while it waits, then connected or why not. Leaving closes the view; a
 * record it already collected still arrives through the `debrid` subscription. */
function SignIn(props: { service: DebridService; account: DebridAccount | undefined; pending: boolean; onClose: (said?: string) => void }) {
  const open = () => liveView<DebridLoginSnapshot>("debridLogin", { service: props.service.id }, true);
  const [view, setView] = createSignal(open());
  onCleanup(() => void view().close());
  // "Get a new code": the old sign-in closes and a new one starts.
  const again = () => {
    void view().close();
    setView(open());
  };
  const phase = (): DebridLoginSnapshot => view().state.data ?? { phase: "starting" };
  const [now, setNow] = createSignal(Date.now());
  const timer = setInterval(() => setNow(Date.now()), 1000);
  onCleanup(() => clearInterval(timer));
  // The clock starts again with each code, so the time left never shows more than the code has.
  createEffect(() => {
    if (phase().phase === "code") setNow(Date.now());
  });
  // Done counts once the record is stored: then the card says Connected.
  createEffect(() => {
    if (phase().phase === "done" && props.account && !props.pending) props.onClose("Connected");
  });
  const failed = () => view().state.status.state === "failed";
  const why = () => {
    const status = view().state.status;
    return status.state === "failed" ? status.reason : null;
  };
  return (
    <div class="dlogin">
      <Show when={phase().phase === "code" && (phase() as Extract<DebridLoginSnapshot, { phase: "code" }>)} keyed>
        {(c) => (
          <div class="pair-grid">
            <Show when={form() !== "phone"}>
              <div class="qrbox">
                <Qr text={c.qr} label={`Sign-in QR code for ${props.service.name}`} />
              </div>
            </Show>
            <div class="pair-steps">
              <p class="step">{form() === "phone" ? "Open" : "Scan the code with your phone, or open"}</p>
              <p class="step mono">{address(c.url)}</p>
              <p class="step">and enter this code:</p>
              <p class="code" aria-label={`Code ${[...c.userCode].join(" ")}`}>
                {c.userCode}
              </p>
              <p class="note small">Good for {waitText(c.expiresMs - now())}</p>
            </div>
          </div>
        )}
      </Show>
      <Show when={phase().phase !== "code"}>
        <p class={`st ${["expired", "denied", "used", "failed"].includes(phase().phase) || failed() ? "bad" : ""}`} role="status">
          <i class="dot" />
          {why() ?? (phase().phase === "done" ? "Saving" : loginWords(phase()))}
        </p>
      </Show>
      <div class="dcard-acts">
        <Show when={phase().phase === "code" && form() === "phone" && (phase() as Extract<DebridLoginSnapshot, { phase: "code" }>)} keyed>
          {(c) => (
            <a class="btn primary" href={c.url} target="_blank" rel="noreferrer">
              <ExternalLink aria-hidden />
              Open the page
            </a>
          )}
        </Show>
        <Show when={["expired", "denied", "used", "failed"].includes(phase().phase) || failed()}>
          <button class="btn primary" data-f={`debrid:${props.service.id}:again`} data-autofocus onClick={again}>
            <RefreshCw aria-hidden />
            Get a new code
          </button>
        </Show>
        {/* Focus lands in the sign-in when it opens: the button that opened it is gone. */}
        <button class="btn" data-f={`debrid:${props.service.id}:cancel`} data-autofocus onClick={() => props.onClose()}>
          {phase().phase === "code" || phase().phase === "starting" ? "Cancel" : "Close"}
        </button>
      </div>
    </div>
  );
}
