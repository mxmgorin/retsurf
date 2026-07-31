// Injected by blob_download.rs once per document load; __PING_URL__ and
// __MAX_BYTES__ are substituted from the Rust constants before injection.
(function () {
  if (window.__retsurfDl) return "already";
  var pending = [];
  var live = new Map();
  var createUrl = URL.createObjectURL.bind(URL);
  var revokeUrl = URL.revokeObjectURL.bind(URL);
  window.__retsurfDl = { pending: pending };

  URL.createObjectURL = function (obj) {
    var url = createUrl(obj);
    if (obj instanceof Blob) live.set(url, obj);
    return url;
  };
  URL.revokeObjectURL = function (url) {
    live.delete(url);
    return revokeUrl(url);
  };

  var pings = 0;
  function ping() {
    // A resource load is the only page-to-embedder signal; the counter keeps
    // the URL out of the image cache, which would swallow a repeat.
    new Image().src = "__PING_URL__?n=" + ++pings;
  }

  // Entries reach Rust as JSON: { name, data } or { error }. See parse_taken.
  function queue(entry) {
    pending.push(entry);
    ping();
  }

  // Opening a type Servo renders itself is a preview, not a download.
  function displayable(blob) {
    var type = (blob.type || "").toLowerCase();
    return type.indexOf("image/") === 0 || type.indexOf("text/") === 0;
  }

  function capture(url, name) {
    var blob = live.get(url);
    if (!blob) return false;
    if (blob.size > __MAX_BYTES__) {
      queue({ error: name + ": " + blob.size + " bytes is over the capture limit" });
      return true;
    }
    var reader = new FileReader();
    reader.onload = function () {
      var text = String(reader.result);
      var comma = text.indexOf(",");
      queue({ name: name, data: comma < 0 ? "" : text.slice(comma + 1) });
    };
    reader.onerror = function () {
      queue({ error: name + ": could not read the blob" });
    };
    reader.readAsDataURL(blob);
    return true;
  }

  document.addEventListener("click", function (event) {
    var a = event.target && event.target.closest && event.target.closest("a[href]");
    if (!a) return;
    var url = a.getAttribute("href") || "";
    if (url.slice(0, 5) !== "blob:") return;
    // Servo lacks the `download` IDL attribute: `a.download = "x"` stays a
    // plain JS property invisible to getAttribute, so read both.
    var asked = a.download || a.getAttribute("download");
    var blob = live.get(url);
    if (!asked && blob && displayable(blob)) return;
    if (capture(url, asked || "download")) event.preventDefault();
  }, true);

  var open = window.open.bind(window);
  window.open = function (url) {
    var blob = typeof url === "string" && url.slice(0, 5) === "blob:" && live.get(url);
    if (blob && !displayable(blob) && capture(url, "download")) {
      return null;
    }
    return open.apply(null, arguments);
  };

  return "ready";
})()
