/** Zap (plans/zap.md 2.2): the loading screen with no title while the core deals a random film or episode, then the
 * player for it from a point in its middle, in place of this page. Back from that play goes where Zap was pressed. */
import { useNavigate, useSearchParams } from "@solidjs/router";
import ArrowLeft from "lucide-solid/icons/arrow-left";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Shuffle from "lucide-solid/icons/shuffle";
import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { op } from "../core/client";
import { BridgeError } from "../core/bridge";
import type { ZapPick } from "../core/generated/ZapPick";
import { goBack } from "../focus/dom";
import { ownState } from "../history";
import { playPath } from "../lib/format";
import { Loading } from "../player/loading";
import words from "../../../shared/stream-choice-words.json";
import "./player.css";

/** Auto zaps in a row before the quiet "Nothing played." (2.3). */
export const ZAP_TRIES = 3;

export const zapWords = {
  zap: "Zap",
  what: "What is this?",
  fromStart: "Start from the beginning",
  openTitle: "Open title",
  again: "Zap again",
  nothingPlayed: "Nothing played.",
  nothingCameUp: "Nothing came up.",
  dial: "Zap from",
  dials: [
    { value: "same", label: "Same", spoken: "Same: what you watch" },
    { value: "near", label: "Near", spoken: "Near: titles like yours" },
    { value: "far", label: "Far", spoken: "Far: what you never open" },
  ],
} as const;

/** The zap page's address: Discover's kind when pressed there, and the auto zaps so far (0 from every press). */
export const zapPath = (kind?: string | null, tries = 0) => {
  const q = new URLSearchParams();
  if (kind) q.set("kind", kind);
  if (tries) q.set("tries", String(tries));
  return `/zap${q.size ? `?${q}` : ""}`;
};

/** A zap's play: its start and its auto zaps ride in the address, so a reload keeps the zap and Back knows it. */
export const zapPlayPath = (p: ZapPick, tries: number, kind?: string | null) => {
  const q = new URLSearchParams({ zap: String(p.startMs) });
  if (tries) q.set("tries", String(tries));
  if (kind) q.set("kind", kind);
  return `${playPath(p.kind, p.itemId, p.videoId)}?${q}`;
};

export default function Zap() {
  const [query] = useSearchParams();
  const navigate = useNavigate();
  const kind = () => (typeof query.kind === "string" ? query.kind : null);
  // Auto zaps so far: from the address on arrival, 0 again from a press here.
  let tries = Number(query.tries) || 0;
  const [miss, setMiss] = createSignal<"noAddons" | "nothingCameUp" | null>(null);
  // Back while it deals: the answer that lands after must not replace the page the viewer went back to.
  let gone = false;
  onCleanup(() => (gone = true));
  const deal = () => {
    setMiss(null);
    op<ZapPick>("zap", kind() ? { kind: kind() } : {}).then(
      (p) => !gone && navigate(zapPlayPath(p, tries, kind()), { replace: true, state: ownState(history.state) }),
      (e) => !gone && setMiss(e instanceof BridgeError && e.code === "noAddons" ? "noAddons" : "nothingCameUp"),
    );
  };
  const again = () => {
    tries = 0;
    deal();
  };
  onMount(deal);
  return (
    <div class="player-scr loading" data-scheme="dark">
      <Loading wait={miss() ? null : "start"} fill={0} logo={undefined} name={zapWords.zap} background={undefined} lines={[]} />
      <Show when={miss()}>
        {(m) => (
          <section class="endcard" role="alert">
            <div class="end-body">
              <p class="st bad">
                <CircleAlert aria-hidden />
                {m() === "noAddons" ? words.empty.noSource.text : zapWords.nothingCameUp}
              </p>
              <div class="acts">
                <Show
                  when={m() === "noAddons"}
                  fallback={
                    <button class="btn primary big" data-f="zap:again" data-autofocus onClick={again}>
                      <Shuffle aria-hidden />
                      {zapWords.again}
                    </button>
                  }
                >
                  <button class="btn primary big" data-f="zap:settings" data-autofocus onClick={() => navigate("/settings", { replace: true })}>
                    {words.empty.noSource.action}
                  </button>
                </Show>
                <button class="btn big" data-f="zap:back" onClick={goBack}>
                  <ArrowLeft aria-hidden />
                  Back
                </button>
              </div>
            </div>
          </section>
        )}
      </Show>
    </div>
  );
}
