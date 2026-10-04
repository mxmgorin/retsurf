// Cut the page's game out and size it to the viewport, reversibly: the game
// element is pinned fixed and scaled by transform over a black backdrop, and
// every inline style touched is saved so turning it off puts the page back
// without a reload. The controller lives on <html> under a symbol, so a
// navigation resets it.
// MODE is "off" or how to scale: "fit", "integer" or "stretch". Every call
// undoes the last one first, so a mode can be swapped in place. A scaling mode
// also runs as a user script before the page has a body, so it polls until a
// game shows up. The result is "on", "waiting" (no game yet) or "off".
(function () {
    const MODE = "GAME_SCALING_MODE";
    // Smaller boxes are ads, avatars and sparklines, not games.
    const MIN_AREA = 100 * 100;
    // The backdrop sits one below the game, both above anything the page has.
    const Z_TOP = 2147483647;
    // How often a page with no game yet is searched again.
    const POLL_MS = 1000;
    const KEY = Symbol.for("retsurf.gameScaling");
    // Each of these on an ancestor makes it the containing block of a fixed
    // descendant, or a stacking context the backdrop could not rise out of.
    const NEUTRAL = {
        transform: "none",
        filter: "none",
        "backdrop-filter": "none",
        perspective: "none",
        contain: "none",
        "will-change": "auto",
        "z-index": "auto",
        opacity: "1",
    };

    // A frame's game is fitted by fitting the frame.
    if (window !== window.top) return "off";
    const root = document.documentElement;
    if (root[KEY]) root[KEY].off();
    if (MODE === "off") return "off";

    let detach = null;
    const attach = () => {
        if (document.body) detach = fitGame();
        return detach !== null;
    };
    const poll = setInterval(() => attach() && clearInterval(poll), POLL_MS);
    root[KEY] = {
        off() {
            clearInterval(poll);
            if (detach) detach();
            delete root[KEY];
        },
    };
    if (attach()) clearInterval(poll);
    return detach ? "on" : "waiting";

    function set(el, props) {
        for (const [name, value] of Object.entries(props)) el.style.setProperty(name, value, "important");
    }

    function setStyle(el, css) {
        if (css === null) el.removeAttribute("style");
        else el.setAttribute("style", css);
    }

    function area(el) {
        const r = el.getBoundingClientRect();
        return r.width * r.height;
    }

    function largest(selector) {
        let best = null;
        for (const el of document.querySelectorAll(selector)) {
            const shown = getComputedStyle(el).visibility !== "hidden" && area(el) >= MIN_AREA;
            if (shown && (!best || area(el) > area(best))) best = el;
        }
        return best;
    }

    // The x and y factors that take a w x h CSS box to the screen under MODE.
    function scale(target, w, h) {
        if (MODE === "stretch") return [innerWidth / w, innerHeight / h];
        const k = Math.min(innerWidth / w, innerHeight / h);
        if (MODE !== "integer") return [k, k];
        // Whole device pixels per game pixel: a canvas counts its backbuffer,
        // anything else its CSS box.
        const dpr = devicePixelRatio || 1;
        const canvas = target instanceof HTMLCanvasElement && target.width > 0 && target.height > 0;
        const bw = canvas ? target.width : w * dpr;
        const bh = canvas ? target.height : h * dpr;
        const n = Math.floor(Math.min((innerWidth * dpr) / bw, (innerHeight * dpr) / bh));
        // Too big for even one: keep the aspect rather than overflow.
        if (n < 1) return [k, k];
        return [(bw * n) / dpr / w, (bh * n) / dpr / h];
    }

    // For the embedder, in device px: [source w, source h, x, y, w, h].
    function report(target, w, h) {
        const dpr = devicePixelRatio || 1;
        const canvas = target instanceof HTMLCanvasElement && target.width > 0 && target.height > 0;
        const r = target.getBoundingClientRect();
        root[KEY].game = [
            canvas ? target.width : w * dpr,
            canvas ? target.height : h * dpr,
            r.left * dpr,
            r.top * dpr,
            r.width * dpr,
            r.height * dpr,
        ];
    }

    // Fit the largest game-sized element; returns what undoes it, or null
    // when the page has none.
    function fitGame() {
        // A cross-origin frame hides its canvas, so the frame is what gets scaled.
        const target = largest("canvas") || largest("iframe, embed, object");
        if (!target) return null;

        const saved = new Map();
        const save = (el) => {
            if (!saved.has(el)) saved.set(el, el.getAttribute("style"));
        };
        for (const el of [root, document.body]) {
            save(el);
            set(el, { overflow: "hidden", margin: "0", background: "#000" });
        }
        for (let el = target.parentElement; el && el !== root; el = el.parentElement) {
            const style = getComputedStyle(el);
            const props = {};
            for (const [name, value] of Object.entries(NEUTRAL)) {
                if (style.getPropertyValue(name) !== value) props[name] = value;
            }
            if (Object.keys(props).length) {
                save(el);
                set(el, props);
            }
        }

        const backdrop = document.createElement("div");
        set(backdrop, { position: "fixed", inset: "0", background: "#000", "z-index": String(Z_TOP - 1) });
        target.before(backdrop);

        // Measured in flow each time, so leaving restores what the game last set.
        let own = target.getAttribute("style");
        let applied = null;
        const observer = new MutationObserver(() => fit());
        const watch = () =>
            observer.observe(target, { attributes: true, attributeFilter: ["style", "width", "height"] });

        function fit() {
            observer.disconnect();
            const current = target.getAttribute("style");
            if (current !== applied) own = current;
            setStyle(target, own);
            const w = target.offsetWidth || target.getBoundingClientRect().width;
            const h = target.offsetHeight || target.getBoundingClientRect().height;
            const [kx, ky] = scale(target, w, h);
            // Scaled, not resized: getBoundingClientRect reports the scaled box,
            // so the game's pointer maths holds.
            set(target, {
                position: "fixed",
                left: "50%",
                top: "50%",
                // offsetWidth counts the border, so the width must too.
                "box-sizing": "border-box",
                width: w + "px",
                height: h + "px",
                margin: "0",
                "max-width": "none",
                "max-height": "none",
                transform: "translate(-50%, -50%) scale(" + kx + ", " + ky + ")",
                "transform-origin": "center",
                "z-index": String(Z_TOP),
            });
            if (MODE === "integer") set(target, { "image-rendering": "pixelated" });
            report(target, w, h);
            applied = target.getAttribute("style");
            observer.takeRecords();
            watch();
        }

        fit();
        addEventListener("resize", fit);
        return () => {
            observer.disconnect();
            removeEventListener("resize", fit);
            backdrop.remove();
            setStyle(target, own);
            for (const [el, css] of saved) setStyle(el, css);
        };
    }
})()
