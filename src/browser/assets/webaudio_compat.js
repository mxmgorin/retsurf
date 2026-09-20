// WebAudio compatibility shims for factory methods Servo does not implement.
// Games rarely feature-detect these and die at init when they are absent, so
// a passthrough facade that keeps audio flowing beats a missing function.
(function () {
  "use strict";
  var Base = window.BaseAudioContext || window.AudioContext;
  if (!Base) { return; }
  var proto = Base.prototype;

  function param(value) {
    return {
      value: value,
      defaultValue: value,
      setValueAtTime: function () { return this; },
      linearRampToValueAtTime: function () { return this; },
      exponentialRampToValueAtTime: function () { return this; },
      setTargetAtTime: function () { return this; },
      setValueCurveAtTime: function () { return this; },
      cancelScheduledValues: function () { return this; }
    };
  }

  // A real GainNode stands in, so graph connections keep working.
  function passthrough(ctx, extra) {
    var node = ctx.createGain();
    for (var k in extra) { node[k] = extra[k]; }
    return node;
  }

  if (!proto.createDynamicsCompressor) {
    proto.createDynamicsCompressor = function () {
      return passthrough(this, {
        threshold: param(-24), knee: param(30), ratio: param(12),
        attack: param(0.003), release: param(0.25), reduction: 0
      });
    };
  }

  if (!proto.createDelay) {
    proto.createDelay = function () {
      return passthrough(this, { delayTime: param(0) });
    };
  }

  if (!proto.createWaveShaper) {
    proto.createWaveShaper = function () {
      return passthrough(this, { curve: null, oversample: "none" });
    };
  }

  if (!proto.createConvolver) {
    proto.createConvolver = function () {
      return passthrough(this, { buffer: null, normalize: true });
    };
  }

  if (!proto.createScriptProcessor) {
    // Fire onaudioprocess ahead of playback and hand each filled buffer to a
    // scheduled AudioBufferSourceNode; the node itself is the output point.
    proto.createScriptProcessor = function (bufferSize, inputChannels, outputChannels) {
      var ctx = this;
      if (!bufferSize) { bufferSize = 4096; }
      if (inputChannels === undefined) { inputChannels = 2; }
      if (outputChannels === undefined) { outputChannels = 2; }
      var node = passthrough(ctx, { bufferSize: bufferSize, onaudioprocess: null });
      var quantum = bufferSize / ctx.sampleRate;
      var ahead = 3 * quantum;
      var next = 0;
      setInterval(function () {
        var handler = node.onaudioprocess;
        if (typeof handler !== "function") { return; }
        var now = ctx.currentTime;
        if (next < now + quantum) { next = now + quantum; }
        while (next < now + ahead) {
          var out = ctx.createBuffer(outputChannels, bufferSize, ctx.sampleRate);
          var input = ctx.createBuffer(Math.max(inputChannels, 1), bufferSize, ctx.sampleRate);
          try {
            handler.call(node, {
              type: "audioprocess", target: node,
              playbackTime: next, inputBuffer: input, outputBuffer: out
            });
          } catch (e) { break; }
          var src = ctx.createBufferSource();
          src.buffer = out;
          src.connect(node);
          src.start(next);
          next += quantum;
        }
      }, Math.max(10, quantum * 500));
      return node;
    };
  }
})();
