(function () {
    const out = [];
    const vw = window.innerWidth, vh = window.innerHeight;
    const els = document.querySelectorAll(
        'a[href], button, input:not([type="hidden"]), select, textarea, summary, ' +
        '[onclick], [role="button"], [role="link"], [role="tab"], [contenteditable="true"]'
    );
    for (const el of els) {
        if (out.length >= 750) break; // 150 hints (5 entries each)
        const r = el.getBoundingClientRect();
        if (r.width < 2 || r.height < 2) continue;
        if (r.bottom < 0 || r.right < 0 || r.top > vh || r.left > vw) continue;
        const s = window.getComputedStyle(el);
        if (s.visibility !== 'visible' || s.pointerEvents === 'none') continue;
        // el.href on an <a> is the resolved absolute URL; restrict to http(s)
        // so "open in new tab" skips javascript:/mailto:/fragment links (those
        // fall back to a normal click). '' marks a non-link clickable.
        const href = (el.tagName === 'A' && /^https?:/i.test(el.href)) ? el.href : '';
        out.push(r.left, r.top, r.width, r.height, href);
    }
    return out;
})()
