//! Address-bar input interpretation: try the text as a URL, then as a bare
//! domain or absolute file path, and finally fall back to the configured search
//! page.

use crate::config::QUERY_MARK;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use url::Url;

/// Everything but RFC 3986's unreserved characters, so `%s` may sit in a path
/// segment as well as a query value.
const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Interpret an input URL.
///
/// If this is not a valid URL, try to "fix" it by adding a scheme or if all else fails,
/// interpret the string as a search term.
pub fn try_into_url<S: AsRef<str>>(request: S, searchpage: &str) -> Option<Url> {
    let request = request.as_ref().trim();

    Url::parse(request)
        .ok()
        .or_else(|| try_as_file(request))
        .or_else(|| try_as_domain(request))
        .or_else(|| try_as_search_page(request, searchpage))
}

fn try_as_file(request: &str) -> Option<Url> {
    if request.starts_with('/') {
        return Url::parse(&format!("file://{}", request)).ok();
    }
    None
}

fn try_as_domain(request: &str) -> Option<Url> {
    fn is_domain_like(s: &str) -> bool {
        !s.starts_with('/') && s.contains('/')
            || (!s.contains(' ') && !s.starts_with('.') && s.split('.').count() > 1)
    }

    if !request.contains(' ') && servo::is_reg_domain(request) || is_domain_like(request) {
        return Url::parse(&format!("https://{}", request)).ok();
    }

    None
}

fn try_as_search_page(request: &str, searchpage: &str) -> Option<Url> {
    if request.is_empty() {
        return None;
    }

    let query = utf8_percent_encode(request, QUERY).to_string();
    Url::parse(&searchpage.replace(QUERY_MARK, &query)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = "https://search.example/?q=%s";

    #[test]
    fn a_query_reaches_the_engine_whole() {
        let url = try_into_url("c++ & #1", SEARCH).expect("a search URL");
        assert_eq!(
            url.as_str(),
            "https://search.example/?q=c%2B%2B%20%26%20%231"
        );
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn a_query_keeps_non_ascii_text() {
        let url = try_into_url("caf\u{e9} au lait", SEARCH).expect("a search URL");
        let (_, q) = url.query_pairs().next().expect("the q pair");
        assert_eq!(q, "caf\u{e9} au lait");
    }
}
