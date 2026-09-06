(function () {
    const el = document.activeElement;
    if (!el) return;
    if ('value' in el) {
        el.value = '';
    } else if (el.isContentEditable) {
        el.textContent = '';
    } else {
        return;
    }
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
})()
