// gamepadconnected for late listeners: Servo fires it once, at the first pad
// gesture, while Chromium fires it when a page starts listening, and pages rely on
// that. A listener added while pads are exposed hears of each one, alone.
(function () {
  "use strict";
  if (typeof GamepadEvent !== "function" || !navigator.getGamepads) { return; }
  var TYPE = "gamepadconnected";

  function replay(listener) {
    setTimeout(function () {
      var pads = navigator.getGamepads();
      for (var i = 0; i < pads.length; i++) {
        if (!pads[i]) { continue; }
        var event = new GamepadEvent(TYPE, { gamepad: pads[i] });
        if (typeof listener === "function") {
          listener.call(window, event);
        } else if (listener && typeof listener.handleEvent === "function") {
          listener.handleEvent(event);
        }
      }
    }, 0);
  }

  var add = window.addEventListener;
  window.addEventListener = function (type, listener, options) {
    add.call(this, type, listener, options);
    if (this === window && type === TYPE && listener) { replay(listener); }
  };

  // The handler attribute's accessor may sit anywhere on the global's chain.
  for (var owner = window; owner; owner = Object.getPrototypeOf(owner)) {
    var handler = Object.getOwnPropertyDescriptor(owner, "on" + TYPE);
    if (!handler) { continue; }
    if (handler.set && handler.configurable) {
      Object.defineProperty(window, "on" + TYPE, {
        configurable: true,
        enumerable: handler.enumerable,
        get: handler.get,
        set: function (value) {
          handler.set.call(this, value);
          if (typeof value === "function") { replay(value); }
        },
      });
    }
    break;
  }
})();
