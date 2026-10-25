//! The license and credits page served at `retsurf:about`, readable offline:
//! the build and its components, the source link, the NOTICE, the README's
//! Credits list and the full GPL text.

use crate::overlay::settings::{about_info, NOTICE};

/// The internal URL the page lives at.
pub const ABOUT_URL: &str = "retsurf:about";

const LICENSE: &str = include_str!("../../LICENSE");
const README: &str = include_str!("../../README.md");

pub fn is_about(url: &url::Url) -> bool {
    url.as_str() == ABOUT_URL
}

pub fn render() -> String {
    let info = about_info();
    let components: String = info
        .components
        .iter()
        .map(|(name, version)| format!("<tr><td>{name}</td><td>{version}</td></tr>"))
        .collect();
    include_str!("assets/about.html")
        .replace("${version}", info.version)
        .replace(
            "${build}",
            &format!("{}, {}", info.git_hash, info.build_date),
        )
        .replace("${components}", &components)
        .replace("${source}", env!("CARGO_PKG_REPOSITORY"))
        .replace("${notice}", &escape(NOTICE))
        .replace("${credits}", &credits(README))
        .replace("${license}", &escape(LICENSE))
}

/// `readme`'s Credits list as `<li>` items, its Markdown links as anchors.
fn credits(readme: &str) -> String {
    let section = readme
        .split_once("\n## Credits\n")
        .map_or("", |(_, rest)| rest);
    let section = section
        .split_once("\n## ")
        .map_or(section, |(list, _)| list);
    let mut items: Vec<String> = Vec::new();
    for line in section.lines() {
        if let Some(item) = line.strip_prefix("- ") {
            items.push(item.to_string());
        } else if let Some(item) = items.last_mut().filter(|_| line.starts_with(' ')) {
            item.push(' ');
            item.push_str(line.trim());
        }
    }
    items
        .iter()
        .map(|item| format!("<li>{}</li>", link_markdown(item)))
        .collect()
}

/// `text` HTML-escaped, its `[label](url)` links turned into anchors.
fn link_markdown(text: &str) -> String {
    let mut html = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let link = rest[open + 1..]
            .split_once("](")
            .and_then(|(label, tail)| tail.split_once(')').map(|(url, after)| (label, url, after)));
        let Some((label, url, after)) = link else {
            break;
        };
        html.push_str(&escape(&rest[..open]));
        html.push_str(&format!(
            "<a href=\"{}\">{}</a>",
            escape(url),
            escape(label)
        ));
        rest = after;
    }
    html.push_str(&escape(rest));
    html
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_come_from_the_readme_list_only() {
        let readme = "# x\n\n## Credits\n\n- By [Servo](https://servo.org)\n- SDL2 for\n  the [Mini](https://a.b/c) <3\n\n## License\n\n- not a credit\n";
        assert_eq!(
            credits(readme),
            "<li>By <a href=\"https://servo.org\">Servo</a></li>\
             <li>SDL2 for the <a href=\"https://a.b/c\">Mini</a> &lt;3</li>"
        );
    }

    #[test]
    fn the_shipped_readme_has_credits() {
        assert!(credits(README).contains("<a href=\"https://servo.org\">Servo</a>"));
    }

    #[test]
    fn page_fills_every_placeholder() {
        assert!(!render().contains("${"));
    }
}
