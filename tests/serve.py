"""Serve tests/pages over HTTP and log the pages' /report beacons.

The pages need a real origin (fetch does not work from file://), and a headless or
handheld run has no way to read an on-screen HUD. So each page GETs /report?... with
its counters and this server timestamps them to stdout.

    python3 tests/serve.py 8099
    # then point [browser] home_page at http://127.0.0.1:8099/<page>.html

    python3 tests/serve.py 8099 0.0.0.0
    # a handheld on the LAN can reach this one; the host firewall has to allow it

Beacons are a no-op against any other static server (they just 404).

The server keeps its connections alive (HTTP/1.1) and answers on a thread per
connection. The default of the class it is built on is neither: it closes after
every response, which races an engine that pools connections — a beacon sent on
one the server had just closed is lost without an error, and a check that never
reported reads as one that stopped passing.

/tone.wav is synthesized at startup (441 Hz, 3 s, stereo, peak 0.5) and served
with Range support, so audio-element.html exercises the seekable-media path the
way a real server would; SimpleHTTPRequestHandler alone answers 200 and Servo
would treat the stream as non-seekable. /tone.mp4 (testsrc2 video + the same
441 Hz tone, H.264 with B-frames + AAC) is built by ffmpeg on first request and
cached in the temp dir; without ffmpeg it 404s.
"""

import http.server
import io
import math
import os
import re
import socketserver
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "pages")
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8099
# Loopback by default; a device on the LAN needs an address it can route to.
BIND = sys.argv[2] if len(sys.argv) > 2 else "127.0.0.1"
START = time.time()

TONE_HZ = 441.0
TONE_SECONDS = 3
TONE_RATE = 44100
TONE_PEAK = 0.5


def tone_wav():
    frames = TONE_RATE * TONE_SECONDS
    data_len = frames * 2 * 2  # stereo, 16-bit
    out = io.BytesIO()
    out.write(b"RIFF")
    out.write(struct.pack("<I", 36 + data_len))
    out.write(b"WAVEfmt ")
    out.write(struct.pack("<IHHIIHH", 16, 1, 2, TONE_RATE, TONE_RATE * 4, 4, 16))
    out.write(b"data")
    out.write(struct.pack("<I", data_len))
    for i in range(frames):
        s = int(math.sin(2 * math.pi * TONE_HZ * i / TONE_RATE) * TONE_PEAK * 32767)
        out.write(struct.pack("<hh", s, s))
    return out.getvalue()


TONE = tone_wav()

# One thread per connection now, so the two things a request can touch outside
# itself need a lock: the cached clip (built once) and the log (a torn line is
# a beacon the reader cannot parse).
MP4_LOCK = threading.Lock()
LOG_LOCK = threading.Lock()


def tone_mp4():
    path = os.path.join(tempfile.gettempdir(), "retsurf-tone.mp4")
    with MP4_LOCK:
        build_tone_mp4(path)
    with open(path, "rb") as f:
        return f.read()


def build_tone_mp4(path):
    if not os.path.exists(path):
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error",
             "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=3",
             "-f", "lavfi", "-i", f"sine=frequency={int(TONE_HZ)}:duration=3",
             "-c:v", "libx264", "-pix_fmt", "yuv420p",
             "-c:a", "aac", "-b:a", "96k", "-ac", "2", "-ar", str(TONE_RATE),
             "-movflags", "+faststart", path],
            check=True,
        )


class Handler(http.server.SimpleHTTPRequestHandler):
    # Keep-alive, so an engine that pools connections never sends a beacon on
    # one this server has already closed. Every response below carries a length
    # or is bodiless, which is what makes a kept connection readable.
    protocol_version = "HTTP/1.1"

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
        if parsed.path == "/tone.wav":
            self._serve_bytes(TONE, "audio/wav")
            return
        if parsed.path == "/tone.mp4":
            try:
                self._serve_bytes(tone_mp4(), "video/mp4")
            except (OSError, subprocess.CalledProcessError) as e:
                self._log(f"tone.mp4 unavailable: {e}")
                self.send_error(404)
            return
        super().do_GET()

    def _serve_bytes(self, body, ctype):
        total = len(body)
        match = re.match(r"bytes=(\d+)-$", self.headers.get("Range") or "")
        start = min(int(match.group(1)), total) if match else 0
        body = body[start:]
        if match:
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{total - 1}/{total}")
        else:
            self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Accept-Ranges", "bytes")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        self._log("HTTP " + fmt % args)

    def _log(self, line):
        with LOG_LOCK:
            print("[%7.2fs] %s" % (time.time() - START, line), flush=True)


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    # A kept-alive connection holds its thread until the client goes away, so
    # the threads have to be daemons for Ctrl-C to end the run.
    daemon_threads = True
    # The pages beacon in bursts; the default of five leaves the rest to the
    # kernel's own queue.
    request_queue_size = 64


with Server((BIND, PORT), Handler) as httpd:
    print(f"serving {ROOT} on http://{BIND}:{PORT}", flush=True)
    httpd.serve_forever()
