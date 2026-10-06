// Junk Surfer: the retsurf mascot rides the sea and jumps the retro junk
// afloat. One input, jump: Space / Up / W, a click or tap (what the pad's A
// sends), or button 0 of any pad. Frames run only while a round or its wipeout
// plays, so an idle page costs nothing. The canvas's `data-title`, if any,
// replaces the game's name on its start screen.
(function () {
  "use strict";
  var canvas = document.getElementById("junk-surfer");
  if (!canvas || !canvas.getContext) return;
  var ctx = canvas.getContext("2d");

  var H = 170;
  var MAX_W = 640;
  var SIDE = 32;
  var SEA = H - 44;

  var GRAVITY = 1900;
  var JUMP = -600;
  var SPEED_START = 230;
  var SPEED_GAIN = 7;
  var SPEED_MAX = 520;
  var GAP_MIN = 0.9;
  var GAP_MAX = 1.9;
  var RIDER_X = 64;
  var RIDER_SCALE = 1.7;
  // The rider's hitbox around its board, smaller than the drawing so a graze
  // does not end the round.
  var HIT_W = 30;
  var HIT_H = 40;
  var RETRY_DELAY = 0.5;
  // Rider motion: lean per px/s of vertical speed, arm sway, landing splash,
  // and the wipeout's tumble.
  var AIR_TILT = 0.0005;
  var SWAY = 0.15;
  var SWAY_RATE = 5;
  var SPLASH_DROPS = 7;
  var SPLASH_LIFE = 0.45;
  var WIPEOUT_TIME = 0.9;
  var WIPEOUT_SPIN = -9;
  var WIPEOUT_LIFT = -320;
  var SCORE_STEP = 12;
  var HI_KEY = "junk-surfer-hi";

  var BG = "#16171a";
  var INK = "#ececea";
  var MUTED = "#8a8f98";
  var TEAL = "#3fb8a0";
  var DEEP_TEAL = "#265750";
  var SEA_FILL = "#1a2a2a";
  var CORAL = "#ff8c69";
  var GOLD = "#ffb870";
  var PLASTIC = "#2f3238";
  var GREY = "#9aa0a8";
  var METAL = "#c9ccd1";
  var BEIGE = "#cfc6b0";

  // The retro junk afloat: its size and how much of its width collides, since
  // a joystick is mostly air around its stick.
  var JUNK = [
    { draw: drawCart, w: 22, h: 28, hitW: 20 },
    { draw: drawFloppy, w: 26, h: 26, hitW: 26 },
    { draw: drawCrt, w: 36, h: 30, hitW: 34 },
    { draw: drawStick, w: 22, h: 34, hitW: 10 },
  ];
  // How deep the junk sits in the water.
  var FLOAT = 4;

  var W = MAX_W;
  var state = "ready";
  var y = 0, vy = 0, speed = SPEED_START, dist = 0, next = 0;
  var junk = [];
  var overAt = 0, last = 0, frame = 0, clock = 0;
  var splash = [];
  // The tumble after a hit: its age, height, climb and spin.
  var wipe = { t: 0, y: 0, vy: 0, spin: 0 };
  var pads = {};
  var hi = readHi();

  function readHi() {
    try {
      return parseInt(localStorage.getItem(HI_KEY), 10) || 0;
    } catch (e) {
      return 0;
    }
  }

  function writeHi() {
    try {
      localStorage.setItem(HI_KEY, String(hi));
    } catch (e) {}
  }

  function fit() {
    var avail = document.documentElement.clientWidth - SIDE;
    W = Math.max(240, Math.min(MAX_W, avail));
    var dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);
    canvas.style.width = W + "px";
    canvas.style.height = H + "px";
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    draw();
  }

  function score() {
    return Math.floor(dist / SCORE_STEP);
  }

  function reset() {
    y = 0;
    vy = 0;
    speed = SPEED_START;
    dist = 0;
    junk = [];
    splash = [];
    clock = 0;
    next = W * 0.6;
  }

  function jump() {
    if (state === "ready") {
      reset();
      start();
    } else if (state === "running") {
      if (y >= 0) vy = JUMP;
    } else if (state === "over" && now() - overAt > RETRY_DELAY) {
      reset();
      start();
    }
  }

  function now() {
    return performance.now() / 1000;
  }

  function start() {
    state = "running";
    last = now();
    if (!frame) frame = requestAnimationFrame(tick);
  }

  function spawn() {
    var kind = JUNK[Math.floor(Math.random() * JUNK.length)];
    junk.push({ x: W + kind.w, kind: kind });
    var gap = GAP_MIN + Math.random() * (GAP_MAX - GAP_MIN);
    next = W + speed * gap;
  }

  function hit(o) {
    var left = RIDER_X - HIT_W / 2;
    var bottom = SEA + y;
    var k = o.kind;
    var x = o.x + (k.w - k.hitW) / 2;
    var top = SEA + FLOAT - k.h;
    return x < left + HIT_W && x + k.hitW > left && bottom > top && bottom - HIT_H < SEA;
  }

  function tick() {
    frame = 0;
    var t = now();
    var dt = Math.min(0.05, t - last);
    last = t;
    if (state === "wipeout") tumble(t, dt);
    else ride(t, dt);
    updateSplash(dt);
    draw();
    var moving = state === "running" || state === "wipeout";
    if (moving && !document.hidden) frame = requestAnimationFrame(tick);
  }

  function ride(t, dt) {
    pollPads();
    clock += dt;
    speed = Math.min(SPEED_MAX, speed + SPEED_GAIN * dt);
    dist += speed * dt;
    var airborne = y < 0;
    vy += GRAVITY * dt;
    y = Math.min(0, y + vy * dt);
    if (y === 0) vy = 0;
    if (airborne && y === 0) splashAt(RIDER_X);

    next -= speed * dt;
    if (next <= W) spawn();
    for (var i = 0; i < junk.length; i++) junk[i].x -= speed * dt;
    junk = junk.filter(function (o) { return o.x + o.kind.w > 0; });

    for (var j = 0; j < junk.length; j++) {
      if (hit(junk[j])) {
        state = "wipeout";
        wipe = { t: 0, y: y, vy: WIPEOUT_LIFT, spin: 0 };
        if (score() > hi) {
          hi = score();
          writeHi();
        }
        return;
      }
    }
  }

  // The rider flips back off the board and sinks; the sea is drawn over it.
  function tumble(t, dt) {
    wipe.t += dt;
    wipe.vy += GRAVITY * dt;
    wipe.y += wipe.vy * dt;
    wipe.spin += WIPEOUT_SPIN * dt;
    if (wipe.t >= WIPEOUT_TIME) {
      state = "over";
      overAt = t;
    }
  }

  function splashAt(x) {
    for (var i = 0; i < SPLASH_DROPS; i++) {
      splash.push({
        x: x + (Math.random() - 0.5) * 24,
        y: SEA - 2,
        vx: -speed * 0.4 - Math.random() * 60,
        vy: -120 - Math.random() * 120,
        life: SPLASH_LIFE,
      });
    }
  }

  function updateSplash(dt) {
    for (var i = 0; i < splash.length; i++) {
      var d = splash[i];
      d.vy += GRAVITY * 0.5 * dt;
      d.x += d.vx * dt;
      d.y += d.vy * dt;
      d.life -= dt;
    }
    splash = splash.filter(function (d) { return d.life > 0; });
  }

  // Edge-triggered, so a held button jumps once.
  function pollPads() {
    if (!navigator.getGamepads) return;
    var list = navigator.getGamepads();
    for (var i = 0; i < list.length; i++) {
      var pad = list[i];
      if (!pad || !pad.buttons.length) continue;
      var down = pad.buttons[0].pressed;
      if (down && !pads[pad.index]) jump();
      pads[pad.index] = down;
    }
  }

  function draw() {
    ctx.fillStyle = BG;
    ctx.fillRect(0, 0, W, H);
    drawSun(W - 70, SEA - 6, 28);
    var sunk = state === "wipeout" || state === "over";
    if (state === "wipeout") drawRider(RIDER_X, SEA + wipe.y, wipe.spin);
    drawSea();
    for (var i = 0; i < junk.length; i++) drawJunk(junk[i]);
    if (!sunk) drawRider(RIDER_X, riderBase(), riderTilt());
    drawSplash();
    drawText();
  }

  // On the sea the board follows the wave under it; in the air it leans with
  // its climb or fall.
  function riderBase() {
    return SEA + y + waveY(RIDER_X, 3, 22, 1);
  }

  function riderTilt() {
    if (y < 0) return vy * AIR_TILT;
    return Math.atan(Math.cos((RIDER_X + dist) / 22) * 3 / 22);
  }

  function drawSplash() {
    ctx.fillStyle = TEAL;
    for (var i = 0; i < splash.length; i++) {
      var d = splash[i];
      ctx.globalAlpha = Math.max(0, d.life / SPLASH_LIFE);
      ctx.beginPath();
      ctx.arc(d.x, d.y, 1.8, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  function drawSun(cx, horizon, r) {
    var cy = horizon - r * 0.4;
    var g = ctx.createLinearGradient(0, cy - r, 0, cy + r);
    g.addColorStop(0, GOLD);
    g.addColorStop(0.3, CORAL);
    g.addColorStop(1, TEAL);
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, 0, W, horizon);
    ctx.clip();
    ctx.fillStyle = g;
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = BG;
    var slats = [[0.586, 0.027], [0.68, 0.039], [0.793, 0.055]];
    for (var i = 0; i < slats.length; i++) {
      ctx.fillRect(cx - r, cy - r + slats[i][0] * 2 * r, 2 * r, slats[i][1] * 2 * r + 0.5);
    }
    ctx.restore();
  }

  function waveY(x, amp, len, phase) {
    return Math.sin((x + dist * phase) / len) * amp;
  }

  function drawSea() {
    ctx.fillStyle = SEA_FILL;
    ctx.beginPath();
    ctx.moveTo(0, H);
    for (var x = 0; x <= W; x += 6) ctx.lineTo(x, SEA + waveY(x, 3, 22, 1));
    ctx.lineTo(W, H);
    ctx.closePath();
    ctx.fill();
    line(DEEP_TEAL, 2, 0, 3, 22, 1);
    line(TEAL, 2, 12, 2.5, 30, 0.6);
  }

  function line(color, width, drop, amp, len, phase) {
    ctx.strokeStyle = color;
    ctx.lineWidth = width;
    ctx.beginPath();
    for (var x = 0; x <= W; x += 6) {
      var py = SEA + drop + waveY(x, amp, len, phase);
      if (x === 0) ctx.moveTo(x, py);
      else ctx.lineTo(x, py);
    }
    ctx.stroke();
  }

  // Each piece draws upward from its bottom-left at the origin, sunk FLOAT into
  // the sea, with foam where it meets the water.
  function drawJunk(o) {
    var k = o.kind;
    ctx.save();
    ctx.translate(o.x, SEA + FLOAT);
    k.draw(k.w, k.h);
    ctx.restore();
    ctx.strokeStyle = TEAL;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(o.x - 3, SEA + 1);
    ctx.lineTo(o.x + k.w + 3, SEA + 1);
    ctx.stroke();
  }

  function box(x, y0, w, h, color) {
    ctx.fillStyle = color;
    ctx.fillRect(x, y0, w, h);
  }

  function drawCart(w, h) {
    ctx.translate(w / 2, 0);
    ctx.rotate(0.15);
    ctx.translate(-w / 2, 0);
    rounded(0, -h, w, h, 2, GREY);
    box(3, -h + 6, w - 6, h * 0.45, CORAL);
    box(3, -h + 6, w - 6, 3, GOLD);
    for (var i = 0; i < 4; i++) box(4 + i * 4, -h + 1.5, 2, 3, PLASTIC);
  }

  function drawFloppy(w, h) {
    rounded(0, -h, w, h, 2, PLASTIC);
    box(w * 0.28, -h, w * 0.44, h * 0.36, METAL);
    box(w * 0.5, -h + 2, w * 0.1, h * 0.26, PLASTIC);
    box(w * 0.14, -h * 0.5, w * 0.72, h * 0.44, INK);
    box(w * 0.14, -h * 0.5, w * 0.72, 3, CORAL);
  }

  function drawCrt(w, h) {
    rounded(0, -h, w, h, 4, BEIGE);
    rounded(4, -h + 4, w - 13, h - 10, 4, BG);
    ctx.globalAlpha = 0.5;
    for (var i = 0; i < 3; i++) box(6, -h + 8 + i * 5, w - 17, 1.5, TEAL);
    ctx.globalAlpha = 1;
    ellipse(w - 4.5, -h + 8, 1.6, 1.6, CORAL);
    ellipse(w - 4.5, -h + 13, 1.6, 1.6, PLASTIC);
  }

  function drawStick(w, h) {
    rounded(0, -9, w, 9, 2, PLASTIC);
    ellipse(w - 5, -6, 2, 1.4, CORAL);
    ctx.strokeStyle = METAL;
    ctx.lineWidth = 3;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.moveTo(w / 2 - 2, -9);
    ctx.lineTo(w / 2, -h + 8);
    ctx.stroke();
    ellipse(w / 2, -h + 6, 5.5, 5.5, CORAL);
  }

  // The banner mascot, in its own units: a handheld with a face on its screen,
  // on a surfboard centered 1.6 units above `base`.
  function drawRider(x, base, tilt) {
    ctx.save();
    ctx.translate(x, base);
    ctx.scale(RIDER_SCALE, RIDER_SCALE);
    ctx.rotate(tilt);
    ellipse(0, -1.6, 11, 1.8, CORAL);

    ctx.strokeStyle = INK;
    ctx.lineWidth = 2.6;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    poly([[-6, -3.2], [-4.5, -7.2], [-4.5, -11.2]]);
    poly([[5, -3.2], [7, -7.2], [5.5, -11.2]]);
    var sway = Math.sin(clock * SWAY_RATE) * SWAY;
    poly(swing([[8, -17.7], [12, -15.2], [15, -12.2]], sway));
    poly(swing([[-7.5, -16.7], [-10.5, -19.2], [-13, -22.7]], -sway));

    // The body leans 15 degrees about its own center, as in the banner.
    ctx.translate(0.5, -17.1);
    ctx.rotate(-0.26);
    ctx.translate(-0.5, 15.5);
    rounded(-10, -21.5, 21, 12, 3, INK);
    rounded(-5.8, -20.4, 12.6, 9.8, 0.6, BG);
    ctx.strokeStyle = TEAL;
    ctx.lineWidth = 0.9;
    ctx.beginPath();
    ctx.moveTo(-1.36, -16.48);
    ctx.quadraticCurveTo(-0.58, -17.85, 0.21, -16.48);
    ctx.moveTo(-0.48, -14.32);
    ctx.quadraticCurveTo(1.09, -12.56, 2.66, -14.32);
    ctx.stroke();
    ellipse(2.75, -16.68, 0.73, 1.08, TEAL);
    ctx.strokeStyle = CORAL;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(-9.34, -15.5);
    ctx.lineTo(-6.46, -15.5);
    ctx.moveTo(-7.9, -16.94);
    ctx.lineTo(-7.9, -14.06);
    ctx.stroke();
    ellipse(8.04, -14.64, 1.2, 1.2, CORAL);
    ellipse(9.76, -16.36, 1.2, 1.2, CORAL);
    ctx.restore();
  }

  function ellipse(cx, cy, rx, ry, color) {
    ctx.save();
    ctx.translate(cx, cy);
    ctx.scale(rx, ry);
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(0, 0, 1, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  function rounded(x, y0, w, h, r, color) {
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.moveTo(x + r, y0);
    ctx.arcTo(x + w, y0, x + w, y0 + h, r);
    ctx.arcTo(x + w, y0 + h, x, y0 + h, r);
    ctx.arcTo(x, y0 + h, x, y0, r);
    ctx.arcTo(x, y0, x + w, y0, r);
    ctx.closePath();
    ctx.fill();
  }

  // The arm turned by `a` about its shoulder, its first point.
  function swing(points, a) {
    var ox = points[0][0], oy = points[0][1];
    var cos = Math.cos(a), sin = Math.sin(a);
    return points.map(function (p) {
      var dx = p[0] - ox, dy = p[1] - oy;
      return [ox + dx * cos - dy * sin, oy + dx * sin + dy * cos];
    });
  }

  function poly(points) {
    ctx.beginPath();
    ctx.moveTo(points[0][0], points[0][1]);
    for (var i = 1; i < points.length; i++) ctx.lineTo(points[i][0], points[i][1]);
    ctx.stroke();
  }

  function pad5(n) {
    var s = String(n);
    while (s.length < 5) s = "0" + s;
    return s;
  }

  function drawText() {
    ctx.font = "bold 14px monospace";
    ctx.textBaseline = "top";
    ctx.textAlign = "right";
    ctx.fillStyle = MUTED;
    var hiText = hi ? "HI " + pad5(hi) + "  " : "";
    ctx.fillText(hiText + pad5(score()), W - 12, 10);
    if (state === "running") return;
    ctx.textAlign = "center";
    ctx.fillStyle = INK;
    ctx.font = "bold 16px monospace";
    var title = state === "ready" ? canvas.dataset.title || "Junk Surfer" : "Wipeout!";
    ctx.fillText(title, W / 2, 36);
    ctx.font = "13px monospace";
    ctx.fillStyle = MUTED;
    var hint = state === "ready" ? "Press A or Space to start" : "Press A or Space to retry";
    ctx.fillText(hint, W / 2, 58);
  }

  document.addEventListener("keydown", function (e) {
    if (e.ctrlKey || e.altKey || e.metaKey) return;
    if (e.code === "Space" || e.code === "ArrowUp" || e.code === "KeyW") {
      e.preventDefault();
      if (!e.repeat) jump();
    }
  });
  // Anywhere on the page: the pad's A clicks wherever its cursor is. No default,
  // so a press does not start a selection over the canvas.
  document.addEventListener("pointerdown", function (e) {
    e.preventDefault();
    jump();
  });
  window.addEventListener("resize", fit);
  document.addEventListener("visibilitychange", function () {
    if (document.hidden && state === "running") {
      state = "over";
      overAt = now();
      draw();
    }
  });

  fit();
})();
