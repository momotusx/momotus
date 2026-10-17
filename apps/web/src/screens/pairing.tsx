import Check from "lucide-solid/icons/check";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import QrCode from "lucide-solid/icons/qr-code";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import X from "lucide-solid/icons/x";
import { createSignal, For, Match, onCleanup, Show, Switch } from "solid-js";
import { Qr } from "../components/qr";
import { liveView } from "../core/client";
import { form } from "../form";
import "./browse.css";
import "./settings.css";

/** The core's pairing snapshot (plans/core-protocol.md, Views). */
type Pairing =
  | { phase: "starting" }
  | { phase: "waiting"; qr: string; expiresMs: number }
  | { phase: "confirm"; code: string }
  | { phase: "joined"; record: unknown }
  | { phase: "expired" }
  | { phase: "failed"; reason: string };

const SERVER_KEY = "mq.syncServer";
const read = (k: string) => {
  try {
    return localStorage.getItem(k) ?? "";
  } catch {
    return "";
  }
};

function Session(props: { server: string; name: string; onDone: () => void }) {
  const view = liveView<Pairing>("pairing", { server: props.server, name: props.name });
  const p = () => view.state.data;
  const [now, setNow] = createSignal(Date.now());
  const tick = setInterval(() => setNow(Date.now()), 1000);
  onCleanup(() => clearInterval(tick));
  const left = () => {
    const x = p();
    return x?.phase === "waiting" ? Math.max(0, Math.round((x.expiresMs - now()) / 1000)) : 0;
  };
  return (
    <Switch
      fallback={
        <p class="st warn live big-state">
          <LoaderCircle aria-hidden class="spin" />
          Contacting {props.server}
        </p>
      }
    >
      <Match when={view.state.status.state === "failed"}>
        <section class="problem">
          <h2>Pairing could not start</h2>
          <p>{(view.state.status as { reason: string }).reason}</p>
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
                Waiting for the other device, code valid for <span class="num">{left()}</span> s
              </p>
            </div>
          </div>
        )}
      </Match>
      <Match when={p()?.phase === "confirm" && (p() as Extract<Pairing, { phase: "confirm" }>)}>
        {(c) => (
          <section class="confirm">
            <p class="step">Does the other device show the same six digits?</p>
            <p class="code num" aria-label={`Code ${c().code.split("").join(" ")}`}>
              <For each={c().code.split("")}>{(d) => <span>{d}</span>}</For>
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
        <section class="summary">
          <h2>
            <Check aria-hidden />
            This device joined the account
          </h2>
          <p class="note small">
            The sync record stays in memory for this session only: a browser has no system keystore. The desktop and TV apps keep it in the keystore.
          </p>
        </section>
      </Match>
      <Match when={p()?.phase === "expired" || p()?.phase === "failed"}>
        <section class="problem">
          <h2>{p()?.phase === "expired" ? "The code expired" : "Pairing stopped"}</h2>
          <p>{p()?.phase === "failed" ? (p() as { reason: string }).reason : "Start again for a new code."}</p>
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
  const [server, setServer] = createSignal(read(SERVER_KEY));
  const [name, setName] = createSignal(`Marquee ${form() === "tv" ? "TV" : form() === "phone" ? "phone" : "desktop"}`);
  const [run, setRun] = createSignal<{ server: string; name: string; n: number } | null>(null);
  const start = (e: Event) => {
    e.preventDefault();
    const s = server().trim();
    if (!s) return;
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
      <form class="install" onSubmit={start}>
        <label class="field-line">
          <span class="kicker">Sync server</span>
          <input type="url" placeholder="https://sync.example.org" value={server()} onInput={(e) => setServer(e.currentTarget.value)} data-f="pair:server" data-autofocus autocomplete="off" spellcheck={false} />
        </label>
        <label class="field-line">
          <span class="kicker">This device</span>
          <input type="text" value={name()} onInput={(e) => setName(e.currentTarget.value)} data-f="pair:name" autocomplete="off" />
        </label>
        <button class="btn primary" type="submit" data-f="pair:start">
          <QrCode aria-hidden />
          Show a code
        </button>
      </form>
      <Show when={run()} keyed>
        {(r) => <Session server={r.server} name={r.name} onDone={() => setRun({ ...r, n: r.n + 1 })} />}
      </Show>
    </main>
  );
}
