// IDBIndex cursor shims: Servo keeps no index data and IDBCursor cannot continue,
// so Emscripten's IDBFS stalls on its first index scan. The cursor is emulated
// over the store, read in key-range chunks so a key cursor never holds every value.
(function () {
  "use strict";
  if (typeof IDBIndex === "undefined" || typeof indexedDB === "undefined") { return; }
  var proto = IDBIndex.prototype;
  var CHUNK_ROWS = 64;
  // Overrides a native openCursor too: Servo's walks primary keys, not the index.

  function keyAt(value, path) {
    var parts = String(path).split(".");
    for (var i = 0; i < parts.length; i++) {
      if (value === null || typeof value !== "object") { return undefined; }
      value = value[parts[i]];
    }
    return value;
  }

  function inRange(key, range) {
    if (range === undefined || range === null) { return true; }
    if (!(range instanceof IDBKeyRange)) { return indexedDB.cmp(key, range) === 0; }
    if (range.lower !== undefined) {
      var lo = indexedDB.cmp(key, range.lower);
      if (lo < 0 || (lo === 0 && range.lowerOpen)) { return false; }
    }
    if (range.upper !== undefined) {
      var hi = indexedDB.cmp(key, range.upper);
      if (hi > 0 || (hi === 0 && range.upperOpen)) { return false; }
    }
    return true;
  }

  function validKey(key) {
    try { indexedDB.cmp(key, key); return true; } catch (e) { return false; }
  }

  function scan(index, range, direction, withValue) {
    var listeners = { success: [], error: [] };
    var request = {
      result: undefined, error: null, readyState: "pending", source: index,
      transaction: index.objectStore.transaction, onsuccess: null, onerror: null,
      addEventListener: function (type, fn) { (listeners[type] = listeners[type] || []).push(fn); },
      removeEventListener: function (type, fn) {
        listeners[type] = (listeners[type] || []).filter(function (f) { return f !== fn; });
      }
    };
    function fire(type) {
      var event = { type: type, target: request, currentTarget: request,
                    preventDefault: function () {}, stopPropagation: function () {} };
      request.readyState = "done";
      if (request["on" + type]) { request["on" + type].call(request, event); }
      (listeners[type] || []).slice().forEach(function (fn) { fn.call(request, event); });
    }
    function fail(source) { request.error = source.error; fire("error"); }

    var store = index.objectStore;
    var rows = [];
    var keysRequest = store.getAllKeys();
    keysRequest.onerror = function () { fail(keysRequest); };
    keysRequest.onsuccess = function () {
      // Servo's getAll* return rows unordered, so chunks are bounded by sorted keys.
      var primaryKeys = keysRequest.result.slice().sort(function (a, b) {
        return indexedDB.cmp(a, b);
      });
      var offset = 0;
      function chunk() {
        if (offset >= primaryKeys.length) { finish(); return; }
        var last = Math.min(offset + CHUNK_ROWS, primaryKeys.length) - 1;
        var bound = IDBKeyRange.bound(primaryKeys[offset], primaryKeys[last]);
        offset = last + 1;
        var chunkKeys = store.getAllKeys(bound);
        chunkKeys.onerror = function () { fail(chunkKeys); };
        chunkKeys.onsuccess = function () {
          var chunkValues = store.getAll(bound);
          chunkValues.onerror = function () { fail(chunkValues); };
          chunkValues.onsuccess = function () {
            // Both reads run the same query, so their rows line up.
            var values = chunkValues.result;
            for (var i = 0; i < values.length; i++) {
              var key = keyAt(values[i], index.keyPath);
              if (key !== undefined && validKey(key) && inRange(key, range)) {
                rows.push({ key: key, primaryKey: chunkKeys.result[i],
                            value: withValue ? values[i] : undefined });
              }
            }
            chunk();
          };
        };
      }
      chunk();
    };

    function finish() {
      rows.sort(function (a, b) {
        return indexedDB.cmp(a.key, b.key) || indexedDB.cmp(a.primaryKey, b.primaryKey);
      });
      if (direction === "prev" || direction === "prevunique") { rows.reverse(); }
      var next = 0;
      function step() {
        if (next >= rows.length) { request.result = null; fire("success"); return; }
        var row = rows[next++];
        request.result = {
          key: row.key, primaryKey: row.primaryKey, source: index, request: request,
          direction: direction || "next", value: row.value,
          continue: function () { Promise.resolve().then(step); },
          advance: function (count) { next += count - 1; Promise.resolve().then(step); }
        };
        fire("success");
      }
      step();
    }
    return request;
  }

  proto.openCursor = function (range, direction) { return scan(this, range, direction, true); };
  proto.openKeyCursor = function (range, direction) { return scan(this, range, direction, false); };
})();
