// The code after # stays on this phone: this script only hands it to the Momotus app, by the
// app's own scheme, for a phone where the link did not open the app (an iPhone without the
// associated domain, a browser that ignored it). It makes no request.
(() => {
  const hash = location.hash;
  const fields = new URLSearchParams(hash.slice(1));
  const code = /^#v=3&/.test(hash) && ["s", "p", "k", "g"].every((name) => fields.get(name));
  if (!code) return;
  const open = document.getElementById("open-app");
  open.setAttribute("href", `momotus://pair${hash}`);
  for (const id of ["open", "open-note"]) document.getElementById(id).hidden = false;
  document.getElementById("no-code").hidden = true;
})();
