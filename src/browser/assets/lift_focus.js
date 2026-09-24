(function () {
    const el = document.activeElement;
    // With no field focused the active element is <body>, whose bottom is the
    // page's end.
    const editable = el && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));
    if (!editable) return;
    const limit = window.innerHeight * (1 - COVERED) - 8;
    const over = el.getBoundingClientRect().bottom - limit;
    if (over > 0) window.scrollBy({ left: 0, top: over });
})()
