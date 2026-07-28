"""Serve tests/pages over HTTP and log the pages' /report beacons.

The pages need a real origin (fetch does not work from file://), and a headless or
handheld run has no way to read an on-screen HUD. So each page GETs /report?... with
its counters and this server timestamps them to stdout.

    python3 tests/serve.py 8099
    # then point [browser] home_page at http://127.0.0.1:8099/<page>.html

Beacons are a no-op against any other static server (they just 404).
"""

import http.server
import os
import socketserver
import sys
import time
import urllib.parse

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "pages")
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8099
START = time.time()


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=ROOT, **kwargs)

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path == "/report":
            fields = urllib.parse.parse_qs(parsed.query)
            self._log(" ".join(f"{k}={v[0]}" for k, v in fields.items()))
            self.send_response(204)
            self.end_headers()
            return
        super().do_GET()

    def log_message(self, fmt, *args):
        self._log("HTTP " + fmt % args)

    def _log(self, line):
        print("[%7.2fs] %s" % (time.time() - START, line), flush=True)


socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer(("127.0.0.1", PORT), Handler) as httpd:
    print(f"serving {ROOT} on http://127.0.0.1:{PORT}", flush=True)
    httpd.serve_forever()
