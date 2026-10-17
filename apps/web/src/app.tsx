import { A, type RouteSectionProps, Route, Router, useLocation, useNavigate } from "@solidjs/router";
import CircleAlert from "lucide-solid/icons/circle-alert";
import Compass from "lucide-solid/icons/compass";
import House from "lucide-solid/icons/house";
import LibraryBig from "lucide-solid/icons/library-big";
import Search from "lucide-solid/icons/search";
import Settings from "lucide-solid/icons/settings";
import { type Component, createEffect, For, on, Show } from "solid-js";
import { bridgeUrl, link } from "./core/client";
import { installFocus, restore } from "./focus/dom";
import { form } from "./form";
import { titleOfVideo } from "./lib/format";
import Home from "./screens/home";
import Onboarding from "./screens/onboarding";
import Player from "./screens/player";
import SettingsScreen from "./screens/settings";
import { Stub } from "./screens/stub";
import Title from "./screens/title";
import Versions from "./screens/versions";

const destinations: { path: string; label: string; icon: Component<{ "aria-hidden"?: boolean }> }[] = [
  { path: "/search", label: "Search", icon: Search },
  { path: "/", label: "Home", icon: House },
  { path: "/discover", label: "Discover", icon: Compass },
  { path: "/library", label: "Library", icon: LibraryBig },
  { path: "/settings", label: "Settings", icon: Settings },
];
const hasNav = (path: string) => destinations.some((d) => d.path === path);

/** Where Back goes when there is no in-app history (a deep link or reload). */
export function parentOf(path: string): string | null {
  const parts = path.split("/").filter(Boolean);
  if (parts[0] === "title" && parts[3] === "versions") return `/${parts.slice(0, 3).join("/")}`;
  if (parts[0] === "play" && parts[1]) return titleOfVideo(decodeURIComponent(parts[1]));
  return path === "/" ? null : "/";
}

let wentBack = false;
export const markBack = () => {
  wentBack = true;
};

function Rail() {
  const location = useLocation();
  return (
    <nav class="rail" aria-label="Main" data-fgroup="rail">
      <div class="brand">Marquee</div>
      <ul>
        <For each={destinations}>
          {(d) => (
            <li>
              <A href={d.path} end class="nav" classList={{ on: location.pathname === d.path }} data-f={`nav:${d.path}`}>
                <d.icon aria-hidden />
                <span>{d.label}</span>
              </A>
            </li>
          )}
        </For>
      </ul>
    </nav>
  );
}

function Tabs() {
  const location = useLocation();
  return (
    <nav class="tabs" aria-label="Main">
      <For each={destinations}>
        {(d) => (
          <A href={d.path} end classList={{ on: location.pathname === d.path }}>
            <d.icon aria-hidden />
            <span>{d.label}</span>
          </A>
        )}
      </For>
    </nav>
  );
}

let focusInstalled = false;

function Shell(props: RouteSectionProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const back = () => {
    const depth = (history.state as { _depth?: number } | null)?._depth ?? 0;
    if (depth > 0) {
      markBack();
      history.back();
      return;
    }
    const parent = parentOf(location.pathname);
    if (!parent) return; // Home is the root: the platform shell owns exit.
    markBack();
    navigate(parent, { replace: true });
  };
  if (!focusInstalled) {
    focusInstalled = true;
    installFocus(back);
  }
  createEffect(
    on(
      () => location.pathname,
      () => {
        const wasBack = wentBack;
        wentBack = false;
        requestAnimationFrame(() => restore(wasBack));
      },
    ),
  );
  const nav = () => hasNav(location.pathname);
  return (
    <div class="app">
      <Show when={nav() && form() !== "phone"}>
        <Rail />
      </Show>
      <div class="screen">{props.children}</div>
      <Show when={nav() && form() === "phone"}>
        <Tabs />
      </Show>
      <Show when={link() === "closed"}>
        <p class="banner bad" role="status">
          <CircleAlert aria-hidden />
          Lost connection to the core at {bridgeUrl}. Restart it, then reload.
        </p>
      </Show>
    </div>
  );
}

export function App() {
  return (
    <Router root={Shell}>
      <Route path="/" component={Home} />
      <Route path="/title/:kind/:id" component={Title} />
      <Route path="/title/:kind/:id/versions/:videoId" component={Versions} />
      <Route path="/play/:videoId" component={Player} />
      <Route path="/welcome" component={Onboarding} />
      <Route path="/settings" component={SettingsScreen} />
      <Route
        path="/search"
        component={() => <Stub title="Search" text="Search across your catalog addons arrives with the core's search view." />}
      />
      <Route
        path="/discover"
        component={() => <Stub title="Discover" text="Browse every catalog by genre and year once the core's discover view lands." />}
      />
      <Route
        path="/library"
        component={() => <Stub title="Library" text="Titles you add from a title page will be listed here." />}
      />
      <Route path="*" component={() => <Stub title="Not found" text="This page does not exist in Marquee." />} />
    </Router>
  );
}
