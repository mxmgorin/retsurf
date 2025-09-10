use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::RcDom;

pub fn parse_html(html: &str) -> RcDom {
    html5ever::parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap()
}

/// Recursive function to print nodes
pub fn print_node(handle: &markup5ever_rcdom::Handle, depth: usize) {
    let node = handle;
    let indent = "  ".repeat(depth);

    match &node.data {
        markup5ever_rcdom::NodeData::Document => {
            // Root node
        }
        markup5ever_rcdom::NodeData::Text { contents } => {
            println!("{}Text: {}", indent, contents.borrow());
        }
        markup5ever_rcdom::NodeData::Element { name, .. } => {
            println!("{}Element: {}", indent, name.local);
        }
        _ => {}
    }

    for child in node.children.borrow().iter() {
        print_node(child, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use crate::{parse_html, print_node};

    #[test]
    fn it_works() {
        let html = r#"
            <html>
                <head><title>Example</title></head>
                <body>
                    <h1>Hello, world!</h1>
                    <p>This is a paragraph.</p>
                </body>
            </html>
        "#;
        let dom = parse_html(html);
        print_node(&dom.document, 0);
    }
}
