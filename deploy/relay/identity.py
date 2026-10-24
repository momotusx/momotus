"""The relay's identity file on the server, /etc/momotus/identity.json (plans/accounts-social-sync.md H5, phase E).

    sudo python3 -I identity.py create             a key-only file: a new subject key, never printed
    sudo python3 -I identity.py merge < env-file   the provider blocks from KEY=VALUE lines on stdin,
                                                   the key, publicUrl and linkHost kept
    python3 -I identity.py --self-test             this file's own rules, on a scratch folder

`merge` refuses a file without a key (a new key would turn every linked identity into a new one,
and the relay refuses to start on another key's fingerprint), keeps the previous file as
identity.json.prev, and replaces the file atomically. Both print only the key's fingerprint.
Standard library only.
"""

from __future__ import annotations

import hashlib
import json
import os
import secrets
import sys
import tempfile
from pathlib import Path

PATH = Path("/etc/momotus/identity.json")
HOST = "momotus.normalisdead.mom"
GRAPH = "v26.0"


def fingerprint(key: str) -> str:
    return hashlib.sha256(bytes.fromhex(key)).hexdigest()[:16]


def write(path: Path, data: dict) -> None:
    tmp = path.with_name(f".{path.name}.tmp")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        json.dump(data, f, indent=2)
        f.write("\n")
    os.replace(tmp, path)


def create(path: Path) -> str:
    if path.exists():
        raise SystemExit(f"{path} exists; merge into it instead")
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    key = secrets.token_hex(32)
    write(path, {"key": key, "publicUrl": f"https://{HOST}", "linkHost": HOST})
    return fingerprint(key)


def providers(env_text: str) -> dict:
    env = dict(line.split("=", 1) for line in env_text.splitlines() if "=" in line and not line.lstrip().startswith("#"))
    env = {k.strip(): v.strip().strip('"').strip("'") for k, v in env.items()}
    blocks: dict = {}
    if env.get("GOOGLE_WEB_CLIENT_ID"):
        blocks["google"] = {"webClientId": env["GOOGLE_WEB_CLIENT_ID"], "webClientSecret": env["GOOGLE_WEB_CLIENT_SECRET"]}
    if env.get("FACEBOOK_APP_ID"):
        blocks["facebook"] = {
            "appId": env["FACEBOOK_APP_ID"],
            "appSecret": env["FACEBOOK_APP_SECRET"],
            "graph": f"https://graph.facebook.com/{GRAPH}",
            "dialog": f"https://www.facebook.com/{GRAPH}/dialog/oauth",
        }
    if not blocks:
        raise SystemExit("no provider values on stdin")
    return blocks


def merge(path: Path, env_text: str) -> str:
    current = json.loads(path.read_text())
    key = current.get("key")
    if not key:
        raise SystemExit(f"{path} has no key: refusing to make a new one")
    before = fingerprint(key)
    # Every other entry (an Apple or OpenID provider set by hand) stays; the
    # file's Google and Facebook blocks are the ones the env file names.
    merged = {k: v for k, v in current.items() if k not in ("google", "facebook")} | providers(env_text)
    write(path.with_name(f"{path.name}.prev"), current)
    write(path, merged)
    after = fingerprint(json.loads(path.read_text())["key"])
    assert after == before, "the key changed"
    return after


def self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "momotus" / "identity.json"
        first = create(path)
        assert oct(path.stat().st_mode & 0o777) == "0o600"
        assert json.loads(path.read_text())["linkHost"] == HOST
        current = json.loads(path.read_text())
        write(path, current | {"oidc": [{"id": "x"}]})
        env = "GOOGLE_WEB_CLIENT_ID=g.apps\nGOOGLE_WEB_CLIENT_SECRET=\"gs\"\n# a comment\nFACEBOOK_APP_ID=1\nFACEBOOK_APP_SECRET='fs'\n"
        assert merge(path, env) == first
        data = json.loads(path.read_text())
        assert data["google"] == {"webClientId": "g.apps", "webClientSecret": "gs"}
        assert data["facebook"]["graph"].endswith("/v26.0") and data["facebook"]["appSecret"] == "fs"
        assert json.loads(path.with_name("identity.json.prev").read_text()).get("google") is None
        assert data["oidc"] == [{"id": "x"}], "entries set by hand stay"
        # A second merge replaces the providers and still keeps the key.
        assert merge(path, "FACEBOOK_APP_ID=2\nFACEBOOK_APP_SECRET=fs2\n") == first
        assert "google" not in json.loads(path.read_text())
        # No key, no merge; an existing file is never created over.
        write(path, {"publicUrl": "https://x", "linkHost": "x"})
        for action in (lambda: merge(path, env), lambda: create(path)):
            try:
                action()
            except SystemExit:
                pass
            else:
                raise AssertionError("a file without a key, or an existing file, was written")
    print("identity.py self-test: ok")


def main(argv: list[str]) -> int:
    match argv[1:]:
        case ["--self-test"]:
            self_test()
        case ["create"]:
            print(f"identity key fingerprint {create(PATH)}")
        case ["merge"]:
            print(f"identity key fingerprint {merge(PATH, sys.stdin.read())} (unchanged)")
        case _:
            print(__doc__, file=sys.stderr)
            return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
