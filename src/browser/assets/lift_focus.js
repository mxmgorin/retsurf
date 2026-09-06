(function () {
    const el = document.activeElement;
    if (!el || !el.getBoundingClientRect) return;
    const limit = window.innerHeight * (1 - COVERED) - 8;
    const over = el.getBoundingClientRect().bottom - limit;
    if (over > 0) window.scrollBy({ left: 0, top: over });
})()
