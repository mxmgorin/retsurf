// Humble's asm.js ports (FTL) save to /user_data but leave mounting it to
// Humble's player page; elsewhere saves stay in memory. Mounts IndexedDB there.
(function () {
  "use strict";
  var MOUNT = "/user_data";
  var DEPENDENCY = "retsurf-user-data";
  var module;

  function persist() {
    if (typeof HUMBLE_API === "undefined" || typeof IDBFS === "undefined") { return; }
    try { FS.mkdir(MOUNT); } catch (e) { /* already there */ }
    // A page that mounts it itself keeps its own.
    if (FS.lookupPath(MOUNT).node.mount.mountpoint === MOUNT) { return; }
    FS.mount(IDBFS, {}, MOUNT);
    addRunDependency(DEPENDENCY);
    FS.syncfs(true, function (err) {
      if (err) { console.warn("retsurf: loading " + MOUNT + " failed: " + err); }
      removeRunDependency(DEPENDENCY);
    });
  }

  function hook(value) {
    if (!value || typeof value !== "object") { return; }
    var steps = [].concat(value.preRun || []);
    if (steps.indexOf(persist) < 0) { steps.push(persist); }
    value.preRun = steps;
  }

  // `var Module = {...}` and the build's own re-assignment both land here.
  Object.defineProperty(window, "Module", {
    configurable: true,
    get: function () { return module; },
    set: function (value) { hook(value); module = value; }
  });
})();
