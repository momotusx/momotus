import "@fontsource-variable/inter/index.css";
import "@fontsource/ibm-plex-mono/latin-400.css";
import "./theme/tokens.css";
import "./theme/app.css";
import { render } from "solid-js/web";
import words from "../../shared/account-words.json";

const root = document.getElementById("root") as HTMLElement;

if (location.pathname === "/auth") {
  // A sign-in's return (core/sign-in.ts): the fragment goes to this origin's own windows only,
  // leaves this page's URL and history at once, and this page never opens the core.
  const fragment = location.hash.slice(1);
  history.replaceState(null, "", "/auth");
  if (fragment) {
    const channel = new BroadcastChannel("mq-auth");
    channel.postMessage(fragment);
    channel.close();
  }
  root.textContent = words.returnClose;
  window.close();
} else {
  // The app first: loading it loads the router, which stamps this entry's depth, and only then
  // history.ts, whose base reads that stamp (else Back from a first page leaves the app).
  void import("./app").then(async ({ App }) => {
    const [{ markBack }, { startFitting }] = await Promise.all([import("./history"), import("./form")]);
    // Registered before the router's own listener so a browser Back is known
    // before the new screen asks whether to restore focus.
    window.addEventListener("popstate", markBack);
    startFitting();
    // The page's own wordmark (index.html) gives way to the app's, which stays
    // until the core has said who is watching.
    root.replaceChildren();
    render(() => <App />, root);
  });
}
