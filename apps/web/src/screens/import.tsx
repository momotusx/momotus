import { A } from "@solidjs/router";
import Check from "lucide-solid/icons/check";
import ExternalLink from "lucide-solid/icons/external-link";
import House from "lucide-solid/icons/house";
import LoaderCircle from "lucide-solid/icons/loader-circle";
import QrCode from "lucide-solid/icons/qr-code";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import { createSignal, For, Match, Show, Switch } from "solid-js";
import { Qr } from "../components/qr";
import { liveView } from "../core/client";
import type { ImportSnapshot } from "../core/generated/ImportSnapshot";
import { form } from "../form";
import "./browse.css";
import "./settings.css";

/** Opening the view starts the sign-in; a new attempt opens it again. */
function Attempt(props: { onRetry: () => void }) {
  const view = liveView<ImportSnapshot>("import");
  const s = () => view.state.data;
  const hostOf = (url: string) => url.replace(/^https?:\/\//, "").replace(/\/$/, "");
  const [otherDevice, setOtherDevice] = createSignal(false);
  return (
    <Switch
      fallback={
        <p class="st warn live big-state">
          <LoaderCircle aria-hidden class="spin" />
          Starting the sign-in
        </p>
      }
    >
      <Match when={view.state.status.state === "failed"}>
        <section class="problem">
          <h2>Import could not start</h2>
          <p>{(view.state.status as { reason: string }).reason}</p>
        </section>
      </Match>
      <Match when={s()?.phase === "waitingForLink" && (s() as Extract<ImportSnapshot, { phase: "waitingForLink" }>)}>
        {(w) => {
          const code = () => <p class="code" aria-label={`Code ${w().code.split("").join(" ")}`}>{w().code}</p>;
          const waiting = () => (
            <p class="st warn live">
              <i class="dot" />
              Waiting for the code to be entered
            </p>
          );
          const again = () => (
            <button class="btn" data-f="import:new" onClick={props.onRetry}>
              <RefreshCw aria-hidden />
              Get a new code
            </button>
          );
          // A phone signs in on itself: the link opens here, and the QR is
          // only for when the sign-in happens on another device.
          return (
            <Show
              when={form() !== "phone"}
              fallback={
                <div class="pair-steps">
                  <p class="step">
                    <b>1.</b> Open <b class="mono">{hostOf(w().linkUrl)}</b> where you are signed in to Stremio.
                  </p>
                  <a class="btn primary" href={w().linkUrl} target="_blank" rel="noopener noreferrer" data-f="import:open">
                    <ExternalLink aria-hidden />
                    Open {hostOf(w().linkUrl)}
                  </a>
                  <p class="step">
                    <b>2.</b> Enter this code there:
                  </p>
                  {code()}
                  {waiting()}
                  <div class="acts">
                    <button class="btn" data-f="import:other" aria-expanded={otherDevice()} onClick={() => setOtherDevice(!otherDevice())}>
                      <QrCode aria-hidden />
                      Use another device
                    </button>
                    {again()}
                  </div>
                  <Show when={otherDevice()}>
                    <div class="qrbox">
                      <Qr text={w().linkUrl} label={`QR code that opens ${hostOf(w().linkUrl)}`} />
                    </div>
                  </Show>
                </div>
              }
            >
              <div class="pair-grid">
                <div class="qrbox">
                  <Qr text={w().linkUrl} label={`QR code that opens ${hostOf(w().linkUrl)}`} />
                </div>
                <div class="pair-steps">
                  <p class="step">
                    <b>1.</b> Scan the code, or open <b class="mono">{hostOf(w().linkUrl)}</b> on a phone or computer where you are signed in to Stremio.
                  </p>
                  <p class="step">
                    <b>2.</b> Enter this code there:
                  </p>
                  {code()}
                  {waiting()}
                  <div class="acts">{again()}</div>
                </div>
              </div>
            </Show>
          );
        }}
      </Match>
      <Match when={s()?.phase === "importing" && (s() as Extract<ImportSnapshot, { phase: "importing" }>)}>
        {(i) => (
          <p class="st warn live big-state">
            <LoaderCircle aria-hidden class="spin" />
            {i().step}
          </p>
        )}
      </Match>
      <Match when={s()?.phase === "done" && (s() as Extract<ImportSnapshot, { phase: "done" }>)}>
        {(d) => (
          <section class="summary">
            <h2>
              <Check aria-hidden />
              Imported from Stremio
            </h2>
            <dl class="facts-grid">
              <dt>Addons</dt>
              <dd class="num">{d().summary.addonsImported}</dd>
              <dt>Titles</dt>
              <dd class="num">{d().summary.titles}</dd>
              <dt>In progress</dt>
              <dd class="num">{d().summary.inProgress}</dd>
              <dt>Watched</dt>
              <dd class="num">{d().summary.watched}</dd>
            </dl>
            <Show when={d().summary.addonsSkipped.length}>
              <p class="note small">Not imported: {d().summary.addonsSkipped.join("; ")}.</p>
            </Show>
            <Show when={d().summary.seriesUnmatched}>
              <p class="note small">{d().summary.seriesUnmatched} series kept their watched marks unmatched: their episode lists could not be loaded.</p>
            </Show>
            <A class="btn primary" href="/" data-f="import:home" data-autofocus>
              <House aria-hidden />
              Go to Home
            </A>
          </section>
        )}
      </Match>
      <Match when={s()?.phase === "expired" || s()?.phase === "failed"}>
        <section class="problem">
          <h2>{s()?.phase === "expired" ? "The code expired" : "Import stopped"}</h2>
          <p>{s()?.phase === "failed" ? (s() as { reason: string }).reason : "Codes last a few minutes. Get a new one and enter it on the other device."}</p>
          <button class="btn primary" data-f="import:retry" data-autofocus onClick={props.onRetry}>
            <RefreshCw aria-hidden />
            Get a new code
          </button>
        </section>
      </Match>
    </Switch>
  );
}

export default function Import() {
  const [attempt, setAttempt] = createSignal(0);
  return (
    <main class="browse page settings-page" data-vscroll>
      <h1>Import from Stremio</h1>
      <p class="lede">
        Brings your addons, library, Continue Watching and watched marks over once. Nothing is sent back to Stremio.
        <Show when={form() === "tv"}> Nothing is typed on this screen.</Show>
      </p>
      <Show when={attempt() + 1} keyed>
        <Attempt onRetry={() => setAttempt(attempt() + 1)} />
      </Show>
    </main>
  );
}
