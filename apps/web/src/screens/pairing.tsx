import Check from "lucide-solid/icons/check";
import Copy from "lucide-solid/icons/copy";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import QrCode from "lucide-solid/icons/qr-code";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Settings2 from "lucide-solid/icons/settings-2";
import { createEffect, createResource, createSignal, Match, on, onCleanup, Show, Switch } from "solid-js";
import { Qr } from "../components/qr";
import { A } from "@solidjs/router";
import { liveView, privateLink } from "../core/client";
import { autoFocusSoon } from "../focus/dom";
import type { PairingSnapshot as Pairing } from "../core/generated/PairingSnapshot";
import { form } from "../form";
import { signInServer } from "../core/sign-in";
import * as sync from "../core/sync";
import { accountWords as aw, fill, PAIRING_LATE_MS, pairingStarting } from "../lib/account";
import { pairingProblem, plainAddress, syncWords } from "../lib/words";
import "./browse.css";
import "./settings.css";
import { isWebOS } from "../lib/webos";

/** A pairing code and its steps (plans/tv-qr-login.md 5.5): the QR and `how`, the next code in its
 * place as each ends, then the join, with nothing asked here (the phone checked this screen). The
 * browser's account slot is held from the start, so no other account can start here meanwhile;
 * `onJoined` gets the approving device's name. */
export function Session(props: { server: string; name: string; onJoined: (by: string | null) => void; how?: string }) {
  // An account that appeared here meanwhile is never replaced: the slot is taken before the first
  // code shows, and a browser that already holds an account shows no code.
  const claim = sync.claimPairing();
  const view = claim ? liveView<Pairing>("pairing", { server: props.server, name: props.name, form: sync.deviceForm() }) : null;
  const p = () => view?.state.data;
  const [refused, setRefused] = createSignal(!claim);
  const [copied, setCopied] = createSignal(false);
  let took = false;
  onCleanup(() => {
    if (claim && !took) sync.endPairing(claim);
  });
  // The code renews itself for as long as this shows (plans/tv-qr-login.md Q5): nothing to press.
  createEffect(() => {
    const s = p();
    if (s?.phase !== "joined" || took || !claim) return;
    took = true;
    // The join is stored as a copy the moment it arrives: the view's store may change it.
    if (sync.joinPaired(JSON.parse(JSON.stringify(s.record)), claim, s.by)) props.onJoined(s.by);
    else setRefused(true);
  });
  // No code yet past PAIRING_LATE_MS (a relay out of reach): one plain line and Try again, which waits once more
  // while the core keeps asking.
  const [late, setLate] = createSignal(false);
  let lateTimer: ReturnType<typeof setTimeout> | undefined;
  const waitForCode = () => {
    setLate(false);
    clearTimeout(lateTimer);
    lateTimer = setTimeout(() => setLate(true), PAIRING_LATE_MS);
  };
  waitForCode();
  onCleanup(() => clearTimeout(lateTimer));
  const starting = () => pairingStarting(late());
  // The step that asks something takes focus: on a TV the remote is never left on a button that went.
  createEffect(
    on(
      () => [p()?.phase ?? view?.state.status.state, late()],
      () => autoFocusSoon(() => document.querySelector<HTMLElement>("main :is(.summary, .problem) [data-autofocus]")),
    ),
  );
  return (
    <Switch
      fallback={
        <Show
          when={starting().retry}
          fallback={
            <p class="st warn live big-state">
              <LoaderCircle aria-hidden class="spin" />
              {starting().text}
            </p>
          }
        >
          {(retry) => (
            <section class="problem" role="status">
              <p>{starting().text}</p>
              <button class="btn" data-f="pair:retry" data-autofocus onClick={waitForCode}>
                <RefreshCw aria-hidden />
                {retry()}
              </button>
            </section>
          )}
        </Show>
      }
    >
      <Match when={refused()}>
        <section class="problem">
          <p>{syncWords.alreadyJoined}</p>
        </section>
      </Match>
      <Match when={view?.state.status.state === "failed"}>
        <section class="problem">
          <h2>Pairing could not start</h2>
          <p>{pairingProblem((view?.state.status as { reason: string }).reason)}</p>
        </section>
      </Match>
      <Match when={p()?.phase === "waiting" && (p() as Extract<Pairing, { phase: "waiting" }>)}>
        {(w) => (
          <div class="pair-grid" classList={{ tv: form() === "tv" }}>
            {/* A new code takes the old one's place, with the same size and no new announcement. */}
            <div class="qrbox" aria-live="off">
              <Qr text={w().qr} label={aw.tvQrLabel} ecc="L" border={4} />
            </div>
            <div class="pair-steps">
              <p class="step">{props.how ?? aw.tvScanHow}</p>
              <Show when={form() !== "tv"}>
                <p class="note small">{syncWords.copyHelp}</p>
                <div class="acts">
                  <button
                    class="btn"
                    data-f="pair:copy"
                    onClick={() =>
                      void navigator.clipboard.writeText(w().qr).then(
                        () => setCopied(true),
                        () => setCopied(false),
                      )
                    }
                  >
                    <Show when={copied()} fallback={<Copy aria-hidden />}>
                      <Check aria-hidden />
                    </Show>
                    {copied() ? syncWords.copied : syncWords.copyCode}
                  </button>
                </div>
              </Show>
            </div>
          </div>
        )}
      </Match>
      <Match when={p()?.phase === "joining" || p()?.phase === "joined"}>
        <p class="st warn live big-state">
          <LoaderCircle aria-hidden class="spin" />
          {aw.approveSigningIn}
        </p>
      </Match>
    </Switch>
  );
}

function PairForm() {
  // The server this device used before: with it, the code shows at once and
  // nothing is typed. The fields live under Advanced, open only when no
  // server is known yet.
  // A remembered address with a user name or password (an older version saved it) is not used.
  // The hosted relay too, so a build that knows it shows the code at once (plans/tv-qr-login.md 5.5).
  const known = signInServer();
  const [server, setServer] = createSignal(known);
  const [name, setName] = createSignal(sync.defaultDeviceName());
  const [run, setRun] = createSignal<{ server: string; name: string; n: number } | null>(known ? { server: known, name: name(), n: 1 } : null);
  const [advanced, setAdvanced] = createSignal(!known);
  // Once joined, the page is the result and Done: no second pairing starts from it.
  const [joined, setJoined] = createSignal<{ by: string | null } | null>(null);
  const [bad, setBad] = createSignal(false);
  const start = (e: Event) => {
    e.preventDefault();
    const s = server().trim();
    // An address with a user name or password is never kept or sent.
    setBad(s !== "" && !plainAddress(s));
    if (!s || bad()) return;
    sync.rememberJoin(s, name().trim() || "Momotus");
    setRun({ server: s, name: name().trim() || "Momotus", n: (run()?.n ?? 0) + 1 });
  };
  return (
    <>
      <Show when={joined()}>
        {(j) => (
          <section class="summary">
            <h2>
              <Check aria-hidden />
              This device joined the account
            </h2>
            <Show when={j().by}>{(by) => <p>{fill(aw.tvSignedInFrom, { device: by() })}</p>}</Show>
            <Show when={!isWebOS()}>
              <p class="note small">{syncWords.tabOnly}</p>
            </Show>
            <div class="acts">
              <A class="btn primary" href="/sync" data-f="pair:done" data-autofocus>
                Done
              </A>
            </div>
          </section>
        )}
      </Show>
      <Show when={!joined() && run()} keyed>
        {(r) => <Session server={r.server} name={r.name} onJoined={(by) => setJoined({ by })} />}
      </Show>
      <Show when={!joined()}>
      <Show when={!known}>
        <p class="note small">Enter your sync server once; this device remembers it.</p>
      </Show>
      <button class="btn disclose" data-f="pair:advanced" aria-expanded={advanced()} onClick={() => setAdvanced(!advanced())}>
        <Settings2 aria-hidden />
        Advanced
      </button>
      <Show when={advanced()}>
        <form class="install" onSubmit={start}>
          <label class="field-line">
            <span class="kicker">Sync server</span>
            <input type="url" placeholder="https://sync.example.org" value={server()} onInput={(e) => setServer(e.currentTarget.value)} data-f="pair:server" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck={false} />
          </label>
          <label class="field-line">
            <span class="kicker">This device</span>
            <input type="text" value={name()} onInput={(e) => setName(e.currentTarget.value)} data-f="pair:name" autocomplete="off" autocorrect="off" />
          </label>
          <Show when={bad()}>
            <p class="st bad" role="status">
              <i class="dot" />
              Enter the address without a user name or password.
            </p>
          </Show>
          <button class="btn primary" type="submit" data-f="pair:start">
            <QrCode aria-hidden />
            Show a code
          </button>
        </form>
      </Show>
      </Show>
    </>
  );
}

export default function PairingScreen() {
  // A device that syncs, or that remembers an account, pairs nothing new: that would leave a
  // ghost of it in the old account. Asked once this browser was checked for a remembered
  // account (never before: a reload would start pairing first), and kept, so this page's own
  // join keeps its summary.
  const [allowed] = createResource(async () => {
    await sync.vaultReady;
    return sync.mayPair();
  });
  return (
    <main class="browse page settings-page" data-vscroll>
      <h1>Pair this device</h1>
      <p class="lede">Joins your sync account so library, progress and addons follow you. Scan the code with your phone's camera.</p>
      <Show
        when={allowed.state === "ready"}
        fallback={
          <p class="st warn" role="status">
            <i class="dot" />
            {syncWords.checking}
          </p>
        }
      >
        <Show
          when={allowed()}
          fallback={
            <section class="problem">
              <p>{!privateLink ? syncWords.plainLink : sync.locked() ? (sync.memoryProblem() ?? syncWords.lockedFirst) : syncWords.alreadyJoined}</p>
              <A class="btn primary" href="/sync" data-f="pair:back" data-autofocus>
                {syncWords.title}
              </A>
            </section>
          }
        >
          <PairForm />
        </Show>
      </Show>
    </main>
  );
}
