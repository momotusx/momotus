/** Zap (plans/zap.md 2.1): an action, never a place. It opens the zap page on a press (OK on TV) and is never part of
 * the TV tab bar's settle. On Discover it zaps the kind Discover shows (Discover keeps it in the prefs); elsewhere it
 * mixes films and series. Not on a kids profile. */
import { A, useLocation } from "@solidjs/router";
import Shuffle from "lucide-solid/icons/shuffle";
import { Show } from "solid-js";
import { prefsView } from "../core/client";
import { profiles } from "../core/profiles";
import { kidsSession } from "../lib/kids";
import { zapPath, zapWords } from "../screens/zap";

export function ZapButton(props: { class?: string; word?: boolean }) {
  const location = useLocation();
  const prefs = prefsView();
  const kind = () => (location.pathname === "/discover" ? prefs.state.data?.discover.kind : null);
  return (
    <Show when={profiles() && !kidsSession(profiles())}>
      <A href={zapPath(kind())} class={props.class ?? "zap-btn"} data-f="nav:zap" aria-label={zapWords.zap} title={zapWords.zap}>
        <Shuffle aria-hidden />
        <Show when={props.word}>
          <span>{zapWords.zap}</span>
        </Show>
      </A>
    </Show>
  );
}
