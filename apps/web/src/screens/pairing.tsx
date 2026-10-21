import Check from "lucide-solid/icons/check";
import Copy from "lucide-solid/icons/copy";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import QrCode from "lucide-solid/icons/qr-code";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Settings2 from "lucide-solid/icons/settings-2";
import X from "lucide-solid/icons/x";
import { createEffect, createResource, createSignal, Match, on, onCleanup, Show, Switch } from "solid-js";
import { Qr } from "../components/qr";
import { A } from "@solidjs/router";
import { liveView, privateLink } from "../core/client";
import { autoFocusSoon } from "../focus/dom";
import type { PairingSnapshot as Pairing } from "../core/generated/PairingSnapshot";
import { form } from "../form";
import * as sync from "../core/sync";
import { pairingProblem, plainAddress, serverHost, syncWords } from "../lib/words";
import "./browse.css";
import "./settings.css";

/** A pairing code and its steps; `how` says where the code is scanned (default: the Account page of a device that syncs). */
export function Session(props: { server: string; name: string; onDone: () => void; onJoined: () => void; how?: string }) {
  const view = liveView<Pairing>("pairing", { server: props.server, name: props.name });
  const p = () => view.state.data;
  // When the code stops working, as a time of day ("1:54 PM").
  const [copied, setCopied] = createSignal(false);
  // The join is stored the moment it arrives, once, as a copy: the view's store may change it.
  let took = false;
  // An account that appeared here meanwhile is never replaced: Yes is refused before the sync
  // server lists this device, and once confirmed nothing else may start one until the record comes.
  const [refused, setRefused] = createSignal(false);
  let claim: symbol | null = null;
  const release = () => {
    if (claim) sync.endPairing(claim);
    claim = null;
  };
  const confirm = () => {
    claim ??= sync.claimPairing();
    if (!claim) {
      setRefused(true);
      return void view.command({ type: "reject" });
    }
    void view.command({ type: "confirm" });
  };
  createEffect(() => {
    const s = p();
    if (s?.phase === "failed" || s?.phase === "expired") release();
    if (s?.phase !== "joined" || took) return;
    took = true;
    if (claim && sync.joinPaired(JSON.parse(JSON.stringify(s.record)), claim)) props.onJoined();
    else setRefused(true);
    claim = null;
  });
  onCleanup(release);
  const until = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  // Each step that asks something takes focus to its answer: on a TV the remote is never left
  // on the form below while "Yes, they match" waits.
  createEffect(
    on(
      () => p()?.phase ?? view.state.status.state,
      () => autoFocusSoon(() => document.querySelector<HTMLElement>("main :is(.confirm, .summary, .problem) [data-autofocus]")),
    ),
  );
  return (
    <Switch
      fallback={
        <p class="st warn live big-state">
          <LoaderCircle aria-hidden class="spin" />
          Contacting {serverHost(props.server)}
        </p>
      }
    >
      <Match when={view.state.status.state === "failed"}>
        <section class="problem">
          <h2>Pairing could not start</h2>
          <p>{pairingProblem((view.state.status as { reason: string }).reason)}</p>
        </section>
      </Match>
      <Match when={p()?.phase === "waiting" && (p() as Extract<Pairing, { phase: "waiting" }>)}>
        {(w) => (
          <div class="pair-grid">
            <div class="qrbox">
              <Qr text={w().qr} label="Pairing QR code" />
            </div>
            <div class="pair-steps">
              <p class="step">{props.how ?? "On a device that already syncs, open Settings, then Account, then Add a device, and scan this code."}</p>
              <p class="st warn live">
                <i class="dot" />
                {/* One text box: the dot's flex row would split the time and the period into columns. */}
                <span>
                  Waiting for the other device. The code works until <span class="num">{until(w().expiresMs)}</span>.
                </span>
              </p>
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
      <Match when={refused()}>
        <section class="problem">
          <p>{syncWords.alreadyJoined}</p>
        </section>
      </Match>
      <Match when={p()?.phase === "confirm" && (p() as Extract<Pairing, { phase: "confirm" }>)}>
        {(c) => (
          <section class="confirm">
            <p class="step">Does the other device show the same six digits?</p>
            <p class="code" aria-hidden="true">
              {c().code}
            </p>
            <p class="sr">{`Code ${c().code.split("").join(" ")}`}</p>
            <div class="acts">
              <button class="btn primary" data-f="pair:yes" data-autofocus onClick={confirm}>
                <Check aria-hidden />
                Yes, they match
              </button>
              <button class="btn" data-f="pair:no" onClick={() => void view.command({ type: "reject" })}>
                <X aria-hidden />
                No, they differ
              </button>
            </div>
          </section>
        )}
      </Match>
      <Match when={p()?.phase === "joined"}>
        <section class="summary">
          <h2>
            <Check aria-hidden />
            This device joined the account
          </h2>
          <p class="note small">{syncWords.tabOnly}</p>
          <div class="acts">
            <A class="btn primary" href="/sync" data-f="pair:done" data-autofocus>
              Done
            </A>
          </div>
        </section>
      </Match>
      <Match when={p()?.phase === "expired" || p()?.phase === "failed"}>
        <section class="problem">
          <h2>{p()?.phase === "expired" ? "The code expired" : "Pairing stopped"}</h2>
          <p>{p()?.phase === "failed" ? pairingProblem((p() as { reason: string }).reason) : "Start again for a new code."}</p>
          <button class="btn primary" data-f="pair:again" data-autofocus onClick={props.onDone}>
            <RefreshCw aria-hidden />
            Start again
          </button>
        </section>
      </Match>
    </Switch>
  );
}

function PairForm() {
  // The server this device used before: with it, the code shows at once and
  // nothing is typed. The fields live under Advanced, open only when no
  // server is known yet.
  // A remembered address with a user name or password (an older version saved it) is not used.
  const known = sync.rememberedServer();
  const [server, setServer] = createSignal(known);
  const [name, setName] = createSignal(sync.defaultDeviceName());
  const [run, setRun] = createSignal<{ server: string; name: string; n: number } | null>(known ? { server: known, name: name(), n: 1 } : null);
  const [advanced, setAdvanced] = createSignal(!known);
  // Once joined, the page is the result and Done: no second pairing starts from it.
  const [joined, setJoined] = createSignal(false);
  const [bad, setBad] = createSignal(false);
  const start = (e: Event) => {
    e.preventDefault();
    const s = server().trim();
    // An address with a user name or password is never kept or sent.
    setBad(s !== "" && !plainAddress(s));
    if (!s || bad()) return;
    sync.rememberJoin(s, name().trim() || "Marquee");
    setRun({ server: s, name: name().trim() || "Marquee", n: (run()?.n ?? 0) + 1 });
  };
  return (
    <>
      <Show when={run()} keyed>
        {(r) => <Session server={r.server} name={r.name} onDone={() => setRun({ ...r, n: r.n + 1 })} onJoined={() => setJoined(true)} />}
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
      <p class="lede">Joins your sync account so library, progress and addons follow you. Nothing is kept until both screens show the same code.</p>
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
