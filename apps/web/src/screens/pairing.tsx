import Check from "lucide-solid/icons/check";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import QrCode from "lucide-solid/icons/qr-code";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Settings2 from "lucide-solid/icons/settings-2";
import X from "lucide-solid/icons/x";
import { createEffect, createSignal, Match, on, Show, Switch } from "solid-js";
import { Qr } from "../components/qr";
import { A } from "@solidjs/router";
import { liveView } from "../core/client";
import { autoFocus } from "../focus/dom";
import type { PairingSnapshot as Pairing } from "../core/generated/PairingSnapshot";
import { form } from "../form";
import { pairingProblem, plainAddress, serverHost } from "../lib/words";
import "./browse.css";
import "./settings.css";

const SERVER_KEY = "mq.syncServer";
const read = (k: string) => {
  try {
    return localStorage.getItem(k) ?? "";
  } catch {
    return "";
  }
};

function Session(props: { server: string; name: string; onDone: () => void; onJoined: () => void }) {
  const view = liveView<Pairing>("pairing", { server: props.server, name: props.name });
  const p = () => view.state.data;
  // When the code stops working, as a time of day ("1:54 PM").
  const until = (ms: number) => new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  // Each step that asks something takes focus to its answer: on a TV the remote is never left
  // on the form below while "Yes, they match" waits.
  createEffect(
    on(
      () => p()?.phase ?? view.state.status.state,
      () => queueMicrotask(() => autoFocus(document.querySelector<HTMLElement>("main :is(.confirm, .summary, .problem) [data-autofocus]"))),
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
              <p class="step">On a device that already syncs, open Settings, then add a device, and scan this code.</p>
              <p class="st warn live">
                <i class="dot" />
                Waiting for the other device. The code works until <span class="num">{until(w().expiresMs)}</span>.
              </p>
            </div>
          </div>
        )}
      </Match>
      <Match when={p()?.phase === "confirm" && (p() as Extract<Pairing, { phase: "confirm" }>)}>
        {(c) => (
          <section class="confirm">
            <p class="step">Does the other device show the same six digits?</p>
            <p class="code" aria-label={`Code ${c().code.split("").join(" ")}`}>
              {c().code}
            </p>
            <div class="acts">
              <button class="btn primary" data-f="pair:yes" data-autofocus onClick={() => void view.command({ type: "confirm" })}>
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
        {(() => {
          props.onJoined();
          return (
            <section class="summary">
              <h2>
                <Check aria-hidden />
                This device joined the account
              </h2>
              <p class="note small">
                The sync record stays in memory for this session only: a browser has no system keystore. The desktop and TV apps keep it in the keystore.
              </p>
              <div class="acts">
                <A class="btn primary" href="/" data-f="pair:done" data-autofocus>
                  Done
                </A>
              </div>
            </section>
          );
        })()}
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

export default function PairingScreen() {
  // The server this device used before: with it, the code shows at once and
  // nothing is typed. The fields live under Advanced, open only when no
  // server is known yet.
  // A remembered address with a user name or password (an older version saved it) is not used.
  const stored = read(SERVER_KEY).trim();
  const known = plainAddress(stored) ? stored : "";
  const [server, setServer] = createSignal(known);
  const [name, setName] = createSignal(`Marquee ${form() === "tv" ? "TV" : form() === "phone" ? "phone" : "desktop"}`);
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
    try {
      localStorage.setItem(SERVER_KEY, s);
    } catch {
      // Remembered for this page only.
    }
    setRun({ server: s, name: name().trim() || "Marquee", n: (run()?.n ?? 0) + 1 });
  };
  return (
    <main class="browse page settings-page" data-vscroll>
      <h1>Pair this device</h1>
      <p class="lede">Joins your sync account so library, progress and addons follow you. Nothing is kept until both screens show the same code.</p>
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
            <input type="url" placeholder="https://sync.example.org" value={server()} onInput={(e) => setServer(e.currentTarget.value)} data-f="pair:server" autocomplete="off" spellcheck={false} />
          </label>
          <label class="field-line">
            <span class="kicker">This device</span>
            <input type="text" value={name()} onInput={(e) => setName(e.currentTarget.value)} data-f="pair:name" autocomplete="off" />
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
    </main>
  );
}
