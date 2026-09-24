// WebAssembly streaming shims: Servo feeds its stream consumer only from a network
// fetch, so a Response over a script-made ReadableStream never settles. Only those
// are buffered and compiled whole; a fetched Response keeps the native path.
(function () {
  "use strict";
  var wasm = window.WebAssembly;
  if (!wasm || typeof wasm.compileStreaming !== "function") { return; }
  var compile = wasm.compile;
  var instantiate = wasm.instantiate;
  var compileStreaming = wasm.compileStreaming;
  var instantiateStreaming = wasm.instantiateStreaming;

  // A script-made Response has an empty url; a fetched one carries its source.
  function fetched(response) {
    return response instanceof Response && response.url !== "";
  }

  // The native checks, made before the body is read: wasm-bindgen falls back to
  // `arrayBuffer()` on a MIME rejection and needs the body still unread.
  function body(source) {
    return Promise.resolve(source).then(function (response) {
      if (!(response instanceof Response)) {
        throw new TypeError("expected Response or Promise resolving to Response");
      }
      var type = (response.headers.get("Content-Type") || "").split(";")[0].trim();
      if (type.toLowerCase() !== "application/wasm") {
        throw new TypeError("Response has unsupported MIME type");
      }
      if (!response.ok) { throw new TypeError("Response does not have ok status"); }
      return response.arrayBuffer();
    });
  }

  wasm.compileStreaming = function (source) {
    return Promise.resolve(source).then(function (response) {
      if (fetched(response)) { return compileStreaming.call(wasm, response); }
      return body(response).then(function (bytes) { return compile.call(wasm, bytes); });
    });
  };
  wasm.instantiateStreaming = function (source, imports) {
    return Promise.resolve(source).then(function (response) {
      if (fetched(response)) { return instantiateStreaming.call(wasm, response, imports); }
      return body(response).then(function (bytes) {
        return instantiate.call(wasm, bytes, imports);
      });
    });
  };
})();
