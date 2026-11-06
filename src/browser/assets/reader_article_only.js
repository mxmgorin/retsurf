if (document.documentElement.dataset.retsurfReader) return "kept";
if (!isProbablyReaderable(document)) return "no-article";
