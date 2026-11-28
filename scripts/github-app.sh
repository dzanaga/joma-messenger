#!/usr/bin/env bash
# Register Joma's GitHub App on your account, from a manifest (D-24 step 4).
#
# Opens a local page with one button; GitHub shows the filled-in app and you press
# "Create GitHub App". GitHub then sends the browser back here and this script keeps the
# app's public parts only (id, slug, client ID) in crates/joma/github-app.json. The
# private key and client secret GitHub hands over once are discarded: the device flow
# needs neither, and both can be regenerated in the app's settings.
#
# Afterwards: open the app's settings page (printed at the end) and tick
# "Enable Device Flow", then install the app on the joma-backup repository only.
set -euo pipefail
cd "$(dirname "$0")/.."
PORT=${PORT:-18766}
OUT=crates/joma/github-app.json
exec python3 -I - "$PORT" "$OUT" <<'EOF'
import http.server, json, secrets, subprocess, sys, urllib.parse, urllib.request, webbrowser, html

port, out = int(sys.argv[1]), sys.argv[2]
state = secrets.token_urlsafe(16)
manifest = {
    "name": "Joma Messenger",
    "url": "https://github.com/dzanaga/joma-messenger",
    "description": "Joma: the ✓ GitHub badge (a public gist joma.json) and the encrypted account backup (a private repo joma-backup).",
    "redirect_url": f"http://127.0.0.1:{port}/done",
    "callback_urls": [f"http://127.0.0.1:{port}/callback"],
    "public": True,
    "request_oauth_on_install": False,
    "default_permissions": {"gists": "write", "contents": "write", "metadata": "read"},
    "default_events": [],
}
page = f"""<!doctype html><meta charset=utf-8><title>Joma GitHub App</title>
<style>body{{font:15px system-ui;max-width:40em;margin:4em auto;padding:0 1em;background:#111;color:#ddd}}
button{{font:inherit;padding:.6em 1.2em;background:#2a6;color:#fff;border:0;cursor:pointer}}code{{color:#8c8}}</style>
<h1>Register Joma's GitHub App</h1>
<p>GitHub will show the app filled in: name <code>Joma Messenger</code> (change it if GitHub says it is taken),
permissions <code>Gists</code> read/write, <code>Contents</code> read/write, <code>Metadata</code> read. No webhook.</p>
<form action="https://github.com/settings/apps/new?state={state}" method=post>
<input type=hidden name=manifest value="{html.escape(json.dumps(manifest))}">
<button>Continue on GitHub</button></form>"""

class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def send(self, body, code=200):
        self.send_response(code); self.send_header("Content-Type", "text/html; charset=utf-8"); self.end_headers()
        self.wfile.write(body.encode())
    def do_GET(self):
        u = urllib.parse.urlparse(self.path); q = urllib.parse.parse_qs(u.query)
        if u.path == "/":
            return self.send(page)
        if u.path != "/done":
            return self.send("not here", 404)
        if q.get("state", [""])[0] != state or "code" not in q:
            return self.send("state mismatch: run the script again", 400)
        code = q["code"][0]
        req = urllib.request.Request(f"https://api.github.com/app-manifests/{urllib.parse.quote(code)}/conversions",
                                     method="POST", headers={"Accept": "application/vnd.github+json", "User-Agent": "joma-setup"})
        app = json.load(urllib.request.urlopen(req))
        keep = {"app_id": app["id"], "slug": app["slug"], "client_id": app["client_id"], "name": app["name"], "owner": app["owner"]["login"]}
        open(out, "w").write(json.dumps(keep, indent=2) + "\n")
        link = f"https://github.com/settings/apps/{keep['slug']}"
        self.send(f"""<!doctype html><meta charset=utf-8><style>body{{font:15px system-ui;max-width:40em;margin:4em auto;background:#111;color:#ddd}}a{{color:#8c8}}</style>
<h1>Done: {html.escape(keep['name'])}</h1><p>Two more clicks on GitHub:</p><ol>
<li><a href="{link}">{link}</a> → tick <b>Enable Device Flow</b> → Save changes.</li>
<li><a href="https://github.com/apps/{keep['slug']}/installations/new">Install it</a> → <b>Only select repositories</b> → joma-backup (no repo yet? do this after your first backup; Joma also links here).</li></ol>
<p>You can close this tab; the terminal says the same.</p>""")
        print(f"\nSaved {out}: {keep}\n\n1. {link}  → tick 'Enable Device Flow' → Save\n2. https://github.com/apps/{keep['slug']}/installations/new  → only select repositories → joma-backup (after the first backup if the repo does not exist yet)\n", flush=True)
        raise SystemExit(0)

srv = http.server.HTTPServer(("127.0.0.1", port), H)
url = f"http://127.0.0.1:{port}/"
print(f"Open {url} (opening it in your browser)…", flush=True)
webbrowser.open(url)
try:
    while True:
        srv.handle_request()
except SystemExit:
    pass
EOF
