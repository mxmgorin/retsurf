// Runs as a Servo user script in every document. A browser keeps the focused
// field in view when the viewport shrinks under it, as when a keyboard rises;
// Servo does not.
(function () {
  window.addEventListener("resize", function () {
    var el = document.activeElement;
    var editable = el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
    if (!editable) return;
    var over = el.getBoundingClientRect().bottom - (window.innerHeight - 8);
    if (over > 0) window.scrollBy({ left: 0, top: over });
  });
})();
