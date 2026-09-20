"""Drive the beacon pages headless and turn their reports into an exit code.

The pages in `pages/` report their counters to `serve.py` (see its header). This
runs one browser per page against that server, waits for the beacons the page
must emit, and checks them — so CI can gate on a running browser rather than on
unit tests alone. Pages that need a gesture get one through xdotool.

    python3 tests/run_pages.py                    # ./target/release/retsurf
    python3 tests/run_pages.py --case io-stress --keep

Needs Xvfb, xdotool and a GL that works without a GPU (`LIBGL_ALWAYS_SOFTWARE=1`);
`video-element` needs ffmpeg, which the server shells out to for its test clip.
"""

import argparse
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time

TESTS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(TESTS)

# The server timestamps every beacon; anything it logs about HTTP itself is noise.
BEACON = re.compile(r"^\[\s*[\d.]+s\] (?!HTTP )(.*)$")
# A probe page's verdict: `=pass` / `=ok` / `=FAIL:<reason>`, name handled separately.
PROBE = re.compile(r"=(pass|ok|fail\S*|FAIL\S*)(?=\s|$)")

WINDOW = (640, 480)
# Window rows the chrome takes, so a page coordinate can be clicked at.
CHROME_HEIGHT = 33


class Beacon:
    """One report line: the raw text, and its `k=v` pairs where they parse."""

    def __init__(self, line):
        self.line = line
        self.fields = {}
        for token in line.split():
            key, sep, value = token.partition("=")
            if sep:
                self.fields[key] = value

    def get(self, key, default=None):
        return self.fields.get(key, default)

    def __contains__(self, key):
        return key in self.fields

    def __repr__(self):
        return self.line


def number(value):
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def merged(beacons):
    """Every field the page reported, last value wins."""
    fields = {}
    for beacon in beacons:
        fields.update(beacon.fields)
    return fields


def probe_results(beacons):
    """`<name>=pass|ok|FAIL:<reason>` pairs, across all of a probe page's lines.

    A check's name can hold spaces, so a pair ends where the next verdict does:
    the name is whatever precedes it, minus any plain `k=v` field in the way.
    """
    results = {}
    for beacon in beacons:
        end = 0
        for match in PROBE.finditer(beacon.line):
            words = beacon.line[end : match.start()].split()
            while words and "=" in words[0]:
                words.pop(0)
            end = match.end()
            if words:
                results[" ".join(words)] = match.group(1)
    return results


def regressions(beacons, expected):
    """Which of the checks that used to pass no longer do."""
    results = probe_results(beacons)
    lost = [name for name in expected if results.get(name, "missing").lower() not in ("pass", "ok")]
    if lost:
        return f"{len(lost)} check(s) stopped passing: " + ", ".join(
            f"{name}={results.get(name, 'missing')}" for name in lost[:5]
        )
    return None


class Driver:
    """Synthetic input. Page coordinates: the chrome is above the page."""

    def __init__(self, display):
        self.env = dict(os.environ, DISPLAY=display)

    def _xdotool(self, *args):
        subprocess.run(["xdotool", *args], env=self.env, check=False)

    def point_at(self, x, y):
        """With no window manager, keys go to the window under the pointer."""
        self._xdotool("mousemove", str(x), str(y + CHROME_HEIGHT))

    def click(self, x, y):
        self._xdotool("mousemove", str(x), str(y + CHROME_HEIGHT), "click", "1")

    def key(self, name):
        self._xdotool("key", name)

    def wheel_down(self, times=6):
        self._xdotool("mousemove", str(WINDOW[0] // 2), str(WINDOW[1] // 2))
        for _ in range(times):
            self._xdotool("click", "5")


class Script:
    """Input steps, one per poll tick. A step returning False is retried."""

    def __init__(self, *steps, after=6.0):
        self.steps = list(steps)
        self.after = after
        self.done = 0

    def step(self, driver, beacons):
        if self.done >= len(self.steps):
            return
        if self.steps[self.done](driver, beacons) is not False:
            self.done += 1


def press_middle(driver, _beacons):
    driver.click(WINDOW[0] // 2, WINDOW[1] // 3)


def press_reported_box(driver, beacons):
    """click.html reports where its button is; aim at that rather than at a guess."""
    box = next((b for b in beacons if "bx" in b), None)
    if box is None:
        return False
    driver.click(
        int(box.get("bx")) + int(box.get("bw")) // 2,
        int(box.get("by")) + int(box.get("bh")) // 2,
    )


def press_keys(driver, _beacons):
    driver.point_at(WINDOW[0] // 2, WINDOW[1] // 2)
    for key in ("a", "Down"):
        driver.key(key)


def press_every_button(driver, _beacons):
    """A column of presses down the page: Tab does not move focus, and a stack of
    buttons has no geometry worth hard-coding."""
    for y in range(20, WINDOW[1] - CHROME_HEIGHT, 12):
        driver.click(60, y)


def scroll_down(driver, _beacons):
    driver.wheel_down()


# What the engine passes today. A check that drops out of this set is a
# regression; one that joins it is not, so new passes need no edit here.
WEBGL2_EXPECTED = [
    "webgl2", "vao", "instancing", "ubo", "sampler", "query", "sync",
    "transformfeedback", "getbufsubdata", "vertex", "fragment", "link",
    "tex3d", "texarray", "storage", "depth", "float", "potmip",
    "mrt", "multisample", "blit", "invalidate",
    "gamepad", "fullscreen", "offscreen", "worker", "wasm", "wasmstream",
    "indexeddb", "localstorage", "audiocontext", "fetchrange", "performancenow",
    "visibility", "plain", "buffer", "open", "write",
]
# Declarative shadow DOM through DOMParser and createContextualFragment is the
# gap this page was written for; everything else passes.
SHREDDIT_EXPECTED = [
    "customElements.define",
    "custom element upgrade via innerHTML",
    "attachShadow",
    "declarative shadow DOM (main parser)",
    "Element.setHTMLUnsafe",
    "Document.parseHTMLUnsafe",
    "insertAdjacentHTML",
    "slot assignment",
    "ElementInternals",
    "fetch + ReadableStream body",
    "IntersectionObserver on shadow-tree target",
]


def io_stress_verdict(beacons):
    last = beacons[-1]
    if last.get("io") != "true":
        return "IntersectionObserver is off; the run proves nothing"
    if not number(last.get("fps")):
        return f"no frames in the last window: {last}"
    if not number(last.get("cb")):
        return f"no IntersectionObserver callbacks: {last}"
    return None


def fixed_overlay_verdict(beacons):
    """The box must survive the restyle; losing it is servo/servo#48081."""
    lost = [b for b in beacons if b.get("after") == "0x0" or b.get("rect") == "0x0"]
    if lost:
        return f"{len(lost)} box(es) lost on restyle, first: {lost[0]}"
    return None


def viewport_verdict(beacons):
    last = beacons[-1]
    if not number(last.get("dpr")):
        return f"device pixel ratio is not a number: {last}"
    if last.get("inner", "0x0") == "0x0" or last.get("screen", "0x0") == "0x0":
        return f"viewport or screen is empty: {last}"
    return None


def raf_pacing_verdict(beacons):
    rate = next(b for b in beacons if "fps" in b)
    if not number(rate.get("fps")):
        return f"no frame rate reported: {rate}"
    return None


def indexeddb_verdict(beacons):
    if not any(b.get("step") == "wrote" for b in beacons):
        return f"never got to a write: {[b.get('step') for b in beacons]}"
    return None


def webaudio_verdict(beacons):
    last = beacons[-1]
    if last.get("decoded") != "ok":
        return f"decodeAudioData failed: {last}"
    if not number(last.get("frames")):
        return f"decoded no frames: {last}"
    return None


def media_verdict(beacons):
    """`<audio>`/`<video>` must reach playback, not merely load."""
    stages = [b.get("stage") for b in beacons]
    if "playing" not in stages:
        return f"never reached playback: {stages}"
    return None


def keys_verdict(beacons):
    """The page must see the press with its code, and see the release too."""
    codes = {b.get("code") for b in beacons if b.get("edge") == "down"}
    missing = {"KeyA", "ArrowDown"} - codes
    if missing:
        return f"never reached the page: {', '.join(sorted(missing))}"
    if not any(b.get("edge") == "up" for b in beacons):
        return "no key release reached the page"
    return None


def webgl_verdict(beacons):
    fields = merged(beacons)
    if fields.get("link") != "true":
        return f"the shader program did not link: {fields}"
    if fields.get("rgba") != fields.get("expect"):
        return f"read back {fields.get('rgba')}, expected {fields.get('expect')}"
    if fields.get("error") != "0":
        return f"GL error {fields.get('error')} after the draw"
    return None


class Case:
    def __init__(
        self, name, url, select, enough, verdict, timeout, drive=None, config="", downloads=0
    ):
        self.name = name
        self.url = url
        self.select = select
        self.enough = enough
        self.verdict = verdict
        self.timeout = timeout
        self.drive = drive
        self.config = config
        self.downloads = downloads


def page_is(name):
    return lambda beacon: beacon.get("page") == name


CASES = [
    # Twenty posts is enough to prove the machinery; the stress numbers are a
    # measurement, not a gate. `contents=1` is the shape that used to hang layout.
    Case(
        name="io-stress",
        url="io-stress.html?posts=20&depth=4&contents=1",
        select=lambda b: "fps" in b and "posts" in b,
        enough=lambda bs: len(bs) >= 2,
        verdict=io_stress_verdict,
        timeout=60,
    ),
    # 7 fresh cases, then 28 restyle pairs. Waiting for all of them also catches
    # a sweep that stops halfway.
    Case(
        name="fixed-overlay",
        url="fixed-overlay.html",
        select=page_is("fixed-overlay"),
        enough=lambda bs: len(bs) >= 35,
        verdict=fixed_overlay_verdict,
        timeout=120,
    ),
    # Sampled twice, and the second sample is the one after the embedder has
    # installed its scale.
    Case(
        name="viewport",
        url="viewport.html",
        select=lambda b: b.get("page") == "viewport" and "dpr" in b,
        enough=lambda bs: len(bs) >= 2,
        verdict=viewport_verdict,
        timeout=45,
    ),
    Case(
        name="raf-pacing",
        url="raf-pacing.html",
        select=lambda b: b.get("page") == "raf-pacing" and "fps" in b,
        enough=lambda bs: len(bs) >= 1,
        verdict=raf_pacing_verdict,
        timeout=45,
    ),
    Case(
        name="indexeddb",
        url="indexeddb.html",
        select=page_is("indexeddb"),
        enough=lambda bs: any(b.get("step") == "wrote" for b in bs),
        verdict=indexeddb_verdict,
        timeout=45,
    ),
    Case(
        name="webaudio-decode",
        url="webaudio-decode.html",
        select=lambda b: "decoded" in b,
        enough=lambda bs: len(bs) >= 1,
        verdict=webaudio_verdict,
        timeout=45,
    ),
    # Our own SDL media backend, end to end: the server synthesizes the clips.
    Case(
        name="audio-element",
        url="audio-element.html",
        select=lambda b: "stage" in b,
        enough=lambda bs: any(b.get("stage") == "playing" for b in bs),
        verdict=media_verdict,
        timeout=60,
    ),
    Case(
        name="video-element",
        url="video-element.html",
        select=lambda b: "stage" in b,
        enough=lambda bs: any(b.get("stage") == "playing" for b in bs),
        verdict=media_verdict,
        timeout=90,
    ),
    # A pixel read back from a WebGL draw: the composite path is not covered, the
    # context and the draw are.
    Case(
        name="webgl",
        url="webgl.html",
        select=page_is("webgl"),
        enough=lambda bs: any("rgba" in b for b in bs),
        verdict=webgl_verdict,
        timeout=60,
    ),
    Case(
        name="webgl2-features",
        url="webgl2-features.html",
        select=page_is("webgl2"),
        enough=lambda bs: any("visibility" in b for b in bs),
        verdict=lambda bs: regressions(bs, WEBGL2_EXPECTED),
        timeout=90,
    ),
    Case(
        name="shreddit-probe",
        url="shreddit-probe.html",
        select=lambda b: b.get("tag") == "probe",
        enough=lambda bs: len(bs) >= 1,
        verdict=lambda bs: regressions(bs, SHREDDIT_EXPECTED),
        timeout=60,
    ),
    # Input from here down.
    Case(
        name="click",
        url="click.html",
        select=page_is("click"),
        enough=lambda bs: any(b.get("clicked") == "1" for b in bs),
        verdict=lambda bs: None
        if any(b.get("hit") == "true" for b in bs)
        else f"the press never landed on the button: {bs[-1]}",
        timeout=60,
        drive=Script(press_middle, press_reported_box),
    ),
    Case(
        name="keys",
        url="keys.html",
        select=page_is("keys"),
        enough=lambda bs: len({b.get("code") for b in bs if b.get("edge") == "down"}) >= 2,
        verdict=keys_verdict,
        timeout=45,
        drive=Script(press_keys),
    ),
    # Seven trigger paths, one button each; the engine has to see a download for
    # every one of them.
    Case(
        name="blob-download",
        url="blob-download.html",
        select=lambda b: b.get("tag") == "blob",
        enough=lambda bs: len({b.get("case") for b in bs}) >= 7,
        verdict=lambda bs: None,
        timeout=90,
        drive=Script(press_every_button, scroll_down, press_every_button),
        config='[downloads]\ndir = "{profile}/downloads"\n',
        downloads=7,
    ),
    # A click is a transient activation, which the request needs; the on-load
    # attempt in the page only records that it is refused without one.
    Case(
        name="fullscreen",
        url="fullscreen.html",
        select=page_is("fullscreen"),
        enough=lambda bs: any(b.get("element") == "set" for b in bs),
        verdict=lambda bs: None
        if any(number(b.get("rect", "0x0").split("x")[1]) >= 400 for b in bs if b.get("element") == "set")
        else "the fullscreen element never took the window",
        timeout=60,
        drive=Script(press_middle),
    ),
    # The feed only grows when the sentinel is scrolled into view, so the wheel
    # is what makes this a test of IntersectionObserver rather than of layout.
    Case(
        name="infinite-scroll",
        url="infinite-scroll.html",
        select=lambda b: "IOcb" in b,
        enough=lambda bs: any(number(b.get("batches")) for b in bs),
        verdict=lambda bs: None
        if number(bs[-1].get("IOhit"))
        else f"the sentinel never intersected: {bs[-1]}",
        timeout=90,
        drive=Script(scroll_down, scroll_down, scroll_down),
    ),
]


def wait_for_port(port, deadline):
    while time.time() < deadline:
        with socket.socket() as probe:
            if probe.connect_ex(("127.0.0.1", port)) == 0:
                return True
        time.sleep(0.2)
    return False


class Harness:
    """Xvfb and the page server, shared by every case."""

    def __init__(self, out, display, port):
        self.out = out
        self.display = display
        self.port = port
        self.log = os.path.join(out, "beacons.log")
        self.xvfb = None
        self.server = None

    def start(self):
        xvfb_log = open(os.path.join(self.out, "xvfb.log"), "wb")
        self.xvfb = subprocess.Popen(
            ["Xvfb", self.display, "-screen", "0", f"{WINDOW[0]}x{WINDOW[1]}x24", "+extension", "GLX"],
            stdout=xvfb_log,
            stderr=subprocess.STDOUT,
        )
        self.server = subprocess.Popen(
            [sys.executable, os.path.join(TESTS, "serve.py"), str(self.port)],
            stdout=open(self.log, "wb"),
            stderr=subprocess.STDOUT,
        )
        deadline = time.time() + 20
        # A taken display fails here rather than as "the page never ran" later.
        while self.xvfb.poll() is None and time.time() < deadline:
            if os.path.exists(f"/tmp/.X11-unix/X{self.display.lstrip(':')}"):
                break
            time.sleep(0.2)
        if self.xvfb.poll() is not None:
            raise SystemExit(f"Xvfb exited; is {self.display} already in use?")
        if not wait_for_port(self.port, deadline):
            raise SystemExit("the page server never came up")

    def stop(self):
        for child in (self.server, self.xvfb):
            if child and child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()

    def beacons_since(self, offset, select):
        """Beacons this case's browser sent, and where to read from next."""
        found = []
        with open(self.log, "r", errors="replace") as log:
            log.seek(offset)
            for line in log:
                if not line.endswith("\n"):  # a half-written line: read it next time
                    break
                offset += len(line)
                match = BEACON.match(line.strip())
                if match:
                    beacon = Beacon(match.group(1))
                    if beacon.fields and select(beacon):
                        found.append(beacon)
        return found, offset


def panic_line(log):
    return next(l for l in log.splitlines() if "panicked at" in l).strip()


def run_case(case, harness, binary, keep):
    profile = os.path.join(harness.out, case.name)
    shutil.rmtree(profile, ignore_errors=True)
    os.makedirs(profile)
    with open(os.path.join(profile, "config.toml"), "w") as config:
        config.write("[browser]\nrestore_tabs = false\n")
        config.write(f'home_page = "http://127.0.0.1:{harness.port}/{case.url}"\n')
        if case.config:
            config.write("\n" + case.config.format(profile=profile))

    env = dict(os.environ)
    env.pop("WAYLAND_DISPLAY", None)
    env.update(
        DISPLAY=harness.display,
        SDL_VIDEODRIVER="x11",
        SDL_AUDIODRIVER="dummy",  # no sound card on a runner
        LIBGL_ALWAYS_SOFTWARE="1",
        RETSURF_DATA_DIR=profile,
        RUST_LOG="warn",
    )
    app_log_path = os.path.join(harness.out, f"{case.name}.log")
    app_log = open(app_log_path, "wb")
    _, offset = harness.beacons_since(0, lambda _: True)  # skip the earlier cases
    app = subprocess.Popen([binary], env=env, stdout=app_log, stderr=subprocess.STDOUT)

    driver = Driver(harness.display)
    beacons = []
    died = None
    started = time.time()
    deadline = started + case.timeout
    try:
        while time.time() < deadline:
            fresh, offset = harness.beacons_since(offset, case.select)
            beacons.extend(fresh)
            if case.enough(beacons):
                break
            if app.poll() is not None:
                died = app.returncode
                break
            if case.drive and time.time() - started >= case.drive.after:
                case.drive.step(driver, beacons)
            time.sleep(0.5)
    finally:
        # The engine can still panic on the way out, so mark where its own run
        # ended: only what it printed before we asked it to quit is the page's.
        live = os.path.getsize(app_log_path)
        if app.poll() is None:
            app.send_signal(signal.SIGTERM)
            try:
                app.wait(timeout=10)
            except subprocess.TimeoutExpired:
                app.kill()
        app_log.close()

    with open(app_log_path, "rb") as log:
        raw = log.read()
    output = raw[:live].decode(errors="replace")
    exiting = raw[live:].decode(errors="replace")
    saved_to = os.path.join(profile, "downloads")
    saved = len(os.listdir(saved_to)) if os.path.isdir(saved_to) else 0
    if not keep:
        shutil.rmtree(profile, ignore_errors=True)

    for beacon in beacons:
        print(f"    {beacon}")
    if "panicked at" in exiting:
        print(f"    note: the engine panicked on the way out ({panic_line(exiting)})")
    if "panicked at" in output:
        return f"the browser panicked: {panic_line(output)}"
    if died is not None:
        return f"the browser exited ({died}) after {len(beacons)} beacon(s)"
    if not beacons:
        return f"no beacons in {case.timeout}s — the page never ran"
    if not case.enough(beacons):
        return f"only {len(beacons)} beacon(s) in {case.timeout}s; the page stopped early"
    if saved < case.downloads:
        return f"{saved} file(s) saved, expected {case.downloads}"
    return case.verdict(beacons)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default=os.path.join(REPO, "target/release/retsurf"))
    parser.add_argument("--case", action="append", help="run only these cases")
    parser.add_argument("--display", default=":77")
    parser.add_argument("--port", type=int, default=8099)
    parser.add_argument("--out", default=os.path.join(tempfile.gettempdir(), "retsurf-pages"))
    parser.add_argument("--keep", action="store_true", help="keep the browser profiles")
    args = parser.parse_args()

    if not os.access(args.binary, os.X_OK):
        raise SystemExit(f"no browser binary at {args.binary}")
    cases = [c for c in CASES if not args.case or c.name in args.case]
    if not cases:
        raise SystemExit(f"no such case: {', '.join(args.case)}")

    shutil.rmtree(args.out, ignore_errors=True)
    os.makedirs(args.out)
    harness = Harness(args.out, args.display, args.port)
    harness.start()
    failures = []
    try:
        for case in cases:
            print(f"== {case.name}")
            started = time.time()
            try:
                failure = run_case(case, harness, args.binary, args.keep)
            except Exception as error:  # one broken case must not hide the rest
                failure = f"the runner itself failed: {error!r}"
            status = "FAIL" if failure else "ok"
            print(f"   {status} ({time.time() - started:.0f}s)" + (f": {failure}" if failure else ""))
            if failure:
                failures.append(f"{case.name}: {failure}")
    finally:
        harness.stop()

    print(f"\nlogs in {args.out}")
    if failures:
        print("\n".join(f"FAIL {f}" for f in failures))
        return 1
    print(f"{len(cases)} page(s) ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
