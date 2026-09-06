if (document.documentElement.dataset.retsurfReader) return "reader";
var article;
try {
    article = new Readability(document.cloneNode(true)).parse();
} catch (e) {
    return "error: " + e;
}
if (!article || !article.content) return "no-article";
var esc = function (s) {
    var d = document.createElement("div");
    d.textContent = s || "";
    return d.innerHTML;
};
var meta = [article.byline, article.siteName].filter(Boolean).join(" · ");
document.documentElement.dataset.retsurfReader = "1";
document.head.innerHTML = '<meta charset="utf-8"><title>' + esc(article.title) +
    '</title><style>__RETSURF_READER_CSS__</style>';
document.body.className = "";
document.body.removeAttribute("style");
document.body.innerHTML = "<article><h1>" + esc(article.title) + "</h1>" +
    (meta ? '<p class="retsurf-meta">' + esc(meta) + "</p>" : "") +
    article.content + "</article>";
window.scrollTo(0, 0);
return "ok";
