(function () {
  var d = window.__retsurfDl;
  if (!d || !d.pending.length) return "";
  return JSON.stringify(d.pending.shift());
})()
