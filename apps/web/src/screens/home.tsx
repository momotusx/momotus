import { A, useNavigate } from "@solidjs/router";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Info from "lucide-solid/icons/info";
import Play from "lucide-solid/icons/play";
import Puzzle from "lucide-solid/icons/puzzle";
import Wrench from "lucide-solid/icons/wrench";
import Star from "lucide-solid/icons/star";
import RefreshCw from "lucide-solid/icons/refresh-cw";
import Sparkle from "lucide-solid/icons/sparkle";
import { createEffect, createSignal, For, on, Show } from "solid-js";
import { Backdrop, Logo } from "../components/art";
import { separators } from "../components/separators";
import { CONTINUE_ROW, Row } from "../components/row";
import { homeView, liveView, op, prefsView, retryHomeRow } from "../core/client";
import type { AddonsSnapshot } from "../core/generated/AddonsSnapshot";
import type { DefaultAddons } from "../core/generated/DefaultAddons";
import { onProfileReset, profiles } from "../core/profiles";
import { cardOffer, defaultsWords, namesLine } from "../lib/defaults";
import { allFailed } from "../lib/rows";
import { form } from "../form";
import type { Tile } from "../core/generated/Tile";
import { playPath, titlePath } from "../lib/format";
import { kindLabel } from "../lib/words";
import "./home.css";

const pageScroll = { top: 0 };
// Another profile's Home opens at the top (web journey review P3-14).
onProfileReset(() => {
  pageScroll.top = 0;
});

/** Where a hero or tile's Play goes: a known video, or the movie itself. */
export function playHref(t: Tile): string | null {
  if (t.videoId) return playPath(t.kind, t.id, t.videoId);
  if (t.kind === "movie") return playPath(t.kind, t.id, t.id);
  return null;
}

/** Once per list version, for a profile that misses some recommended
 * addons: Review opens the step for those, Not now only closes it. Both
 * mark the version seen (synced), so it shows on no other device. */
function DefaultsCard() {
  const addons = liveView<AddonsSnapshot>("addons");
  const prefs = prefsView();
  const [defaults, setDefaults] = createSignal<DefaultAddons | null>(null);
  // Read again when the list changes (the store keeps its objects, so by value).
  createEffect(
    on(
      () => addons.state.data?.addons.map((a) => `${a.url} ${a.enabled}`).join("|"),
      () => void op<DefaultAddons>("defaultAddons").then(setDefaults, () => setDefaults(null)),
    ),
  );
  const kids = () => profiles()?.profiles.find((p) => p.id === profiles()?.active)?.kids ?? false;
  const offer = () => cardOffer(defaults(), prefs.state.data?.defaultsSeen, kids());
  const seen = () => void op("setPrefs", { patch: { defaultsSeen: defaults()?.version } }).catch(() => undefined);
  return (
    <Show when={offer()}>
      {(names) => (
        <section class="defaults-card" aria-label={defaultsWords.cardTitle} data-fgroup="defaults-card">
          <Sparkle aria-hidden class="dc-icon" />
          <div class="dc-text">
            <h2>{defaultsWords.cardTitle}</h2>
            <p>{namesLine(names())}</p>
          </div>
          <div class="acts">
            <A class="btn" href="/welcome?step=2&review=1" data-f="home:defaults:review" onClick={seen}>
              Review
            </A>
            <button class="btn" data-f="home:defaults:later" onClick={seen}>
              Not now
            </button>
          </div>
        </section>
      )}
    </Show>
  );
}

export default function Home() {
  const home = homeView();
  // A profile that never had an addon list starts at the welcome steps.
  const navigate = useNavigate();
  const addons = liveView<AddonsSnapshot>("addons");
  createEffect(() => {
    if (addons.state.data?.fresh) navigate("/welcome", { replace: true });
  });
  const [page, setPage] = createSignal<HTMLElement | null>(null);
  const hasContinue = () => home.rows[0]?.rowId === CONTINUE_ROW;
  const empty = () => home.status.state === "open" && home.rows.length === 0;
  const hero = () => home.hero;
  // Rows still on their way: only then is a missing hero still coming. When
  // every row has answered and none gave a backdrop (offline, posters only),
  // the hero closes instead of shimmering for good.
  const waiting = () => home.status.state === "opening" || home.rows.some((r) => r.state.state === "loading" || r.state.state === "slow");

  const attach = (el: HTMLElement) => {
    setPage(el);
    if (pageScroll.top) queueMicrotask(() => el.scrollTo({ top: pageScroll.top }));
  };

  return (
    <div class="home">
      <main class="page" data-vscroll ref={attach} onScroll={(e) => (pageScroll.top = e.currentTarget.scrollTop)}>
        <section class="hero" classList={{ closed: !hero() && !waiting() }} data-reveal-top aria-label="Featured">
          <Backdrop src={hero()?.tile.background ?? null} class="hero-bd" />
          <Show when={form() === "phone"}>
            <header class="ph-top">
              <span class="brand">Marquee</span>
            </header>
          </Show>
          <div class="hero-body">
          <Show
            when={hero()}
            fallback={
              <Show when={waiting()}>
                <div class="hero-skel" aria-hidden="true">
                  <div class="skel" />
                  <div class="skel" />
                </div>
              </Show>
            }
          >
            {(h) => (
              <>
                <p class="reason eyebrow">{h().reason}</p>
                <Logo src={h().tile.logo} name={h().tile.name} class="hero-logo" />
                <p class="meta" ref={separators}>
                  <Show when={h().tile.rating}>
                    <span class="rate num">
                      <Star aria-hidden />
                      {h().tile.rating}
                    </span>
                    <b aria-hidden="true">·</b>
                  </Show>
                  <Show when={h().tile.releaseInfo} fallback={kindLabel(h().tile.kind)}>
                    <span class="num">{h().tile.releaseInfo}</span>
                  </Show>
                  <Show when={h().runtime}>
                    <b aria-hidden="true">·</b>
                    <span class="num">{h().runtime}</span>
                  </Show>
                  <Show when={h().genres.length}>
                    <b aria-hidden="true">·</b>
                    <span>{h().genres.join(", ")}</span>
                  </Show>
                </p>
                <Show when={h().description}>
                  <p class="hero-syn">{h().description}</p>
                </Show>
                <div class="acts">
                  <Show when={playHref(h().tile)}>
                    {(href) => (
                      <A class="btn primary" href={href()} data-f="hero:play" aria-label={`Play ${h().tile.name}`} data-autofocus={hasContinue() ? undefined : ""}>
                        <Play aria-hidden class="fill" />
                        Play
                      </A>
                    )}
                  </Show>
                  <A
                    class="rbtn"
                    href={titlePath(h().tile.kind, h().tile.id)}
                    data-f="hero:details"
                    aria-label={`Details for ${h().tile.name}`}
                    data-autofocus={hasContinue() || playHref(h().tile) ? undefined : ""}
                  >
                    <span class="ring">
                      <Info aria-hidden />
                    </span>
                    <span class="lbl">Details</span>
                  </A>
                </div>
              </>
            )}
          </Show>
          </div>
        </section>

        <Show when={home.status.state === "failed" && home.status}>
          {(s) => (
            <section class="problem home-problem">
              <CircleAlert aria-hidden class="problem-icon" />
              <h2>Marquee cannot reach its core</h2>
              <p>{"reason" in s() ? (s() as { reason: string }).reason : ""}</p>
              <button class="btn primary" data-f="home:retry" data-autofocus onClick={() => location.reload()}>
                <RefreshCw aria-hidden />
                Try again
              </button>
            </section>
          )}
        </Show>

        <Show when={empty()}>
          <section class="problem home-problem">
            <h2>Home fills up from your addons</h2>
            <p>{defaultsWords.emptyHome}</p>
            <div class="acts">
              {/* Past first run: the recommended addons to switch, not the first-run doors. */}
              <A class="btn primary" href={addons.state.data?.fresh ? "/welcome" : "/welcome?step=2&review=1"} data-f="home:setup" data-autofocus>
                <Wrench aria-hidden />
                Set up Marquee
              </A>
              <A class="btn" href="/addons" data-f="home:addons">
                <Puzzle aria-hidden />
                Add an addon
              </A>
            </div>
          </section>
        </Show>

        <Show
          when={!home.hero && allFailed(home.rows)}
          fallback={
            <>
              {/* Under Continue Watching, or first when there is none; an empty
                  Home already offers Set up Marquee, so not beside it. */}
              <Show when={!hasContinue() && !empty()}>
                <DefaultsCard />
              </Show>
              <For each={home.rows}>
                {(row, i) => (
                  <>
                    <Row row={row} autofocus={i() === 0 && row.rowId === CONTINUE_ROW} root={page} onRetry={(rowId) => void retryHomeRow(rowId)?.catch(() => undefined)} />
                    <Show when={i() === 0 && row.rowId === CONTINUE_ROW}>
                      <DefaultsCard />
                    </Show>
                  </>
                )}
              </For>
            </>
          }
        >
          <section class="problem home-problem">
            <CircleAlert aria-hidden class="problem-icon" />
            <h2>Home could not load</h2>
            <p>None of your addons answered. Check the network, then try again.</p>
            <button class="btn primary" data-f="home:retry-lanes" data-autofocus onClick={() => location.reload()}>
              <RefreshCw aria-hidden />
              Try again
            </button>
          </section>
        </Show>
      </main>
    </div>
  );
}
