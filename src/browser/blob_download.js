// Runs as a Servo user script in every document, before the page's own scripts;
// __PING_URL__ and __MAX_BYTES__ are substituted from blob_download.rs.
(function () {
  if (window.__retsurfDl) return "already";
  // TAKE_JS drains only the top window's queue, so frames deliver there; a
  // cross-origin frame can't reach it, so its clicks are left alone.
  var pending = [];
  try {
    if (window.top !== window) {
      var top = window.top.__retsurfDl;
      if (!top) return "no top queue";
      pending = top.pending;
    }
  } catch (e) {
    return "cross-origin frame";
  }
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

  // Entries reach Rust as JSON: { name, data }, { url, name } or { error }.
  function queue(entry) {
    pending.push(entry);
    ping();
  }

  // Opening a type Servo renders itself is a preview, not a download.
  function displayable(type) {
    type = (type || "").toLowerCase();
    return type.indexOf("image/") === 0 || type.indexOf("text/") === 0;
  }

  function captureBlob(blob, name) {
    if (blob.size > __MAX_BYTES__) {
      queue({ error: name + ": " + blob.size + " bytes is over the capture limit" });
      return;
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
  }

  // The bytes are already in the URL; fetch is just the decoder.
  function captureData(url, name) {
    fetch(url).then(function (r) { return r.blob(); }).then(
      function (blob) { captureBlob(blob, name); },
      function () { queue({ error: name + ": could not decode the data URL" }); }
    );
  }

  function dataType(href) {
    var m = /^data:([^;,]*)/i.exec(href);
    return m ? m[1] : "";
  }

  // Servo lacks the `download` IDL attribute, so `a.download = "x"` stays a
  // plain JS property invisible to getAttribute: read both.
  function askedName(a) {
    return String(a.download || a.getAttribute("download") || "");
  }
  function wantsDownload(a) {
    return !!askedName(a) || a.hasAttribute("download");
  }

  document.addEventListener("click", function (event) {
    var a = event.target && event.target.closest && event.target.closest("a[href]");
    if (!a) return;
    var href = a.getAttribute("href") || "";
    var name = askedName(a) || "download";
    if (href.slice(0, 5) === "blob:") {
      var blob = live.get(href);
      if (!blob) return;
      if (!wantsDownload(a) && displayable(blob.type)) return;
      event.preventDefault();
      captureBlob(blob, name);
    } else if (href.slice(0, 5) === "data:") {
      if (!wantsDownload(a) && displayable(dataType(href))) return;
      event.preventDefault();
      captureData(href, name);
    } else if (wantsDownload(a) && /^https?:/i.test(a.href)) {
      // Hand the resolved URL to the embedder's own fetch.
      event.preventDefault();
      queue({ url: a.href, name: askedName(a) });
    }
  }, true);

  var open = window.open.bind(window);
  window.open = function (url) {
    if (typeof url === "string") {
      var blob = url.slice(0, 5) === "blob:" && live.get(url);
      if (blob && !displayable(blob.type)) {
        captureBlob(blob, "download");
        return null;
      }
      if (url.slice(0, 5) === "data:" && !displayable(dataType(url))) {
        captureData(url, "download");
        return null;
      }
    }
    return open.apply(null, arguments);
  };

  return "ready";
})()
