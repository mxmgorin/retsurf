//! The text clipboard, through SDL: the system's where the video driver has one,
//! an in-process string where it has none (KMSDRM). Page fields reach it as
//! Servo's [`servo::ClipboardDelegate`], the chrome's own fields directly, so a
//! copy in one pastes in the other.

use sdl2::clipboard::ClipboardUtil;

pub struct Clipboard(ClipboardUtil);

impl Clipboard {
    pub fn new(clipboard: ClipboardUtil) -> Self {
        Self(clipboard)
    }

    /// The clipboard's text; empty when it holds none or cannot be read.
    pub fn text(&self) -> String {
        self.0.clipboard_text().unwrap_or_else(|e| {
            log::debug!("clipboard: read failed: {e}");
            String::new()
        })
    }

    pub fn set_text(&self, text: &str) {
        // SDL takes a C string, which cannot carry a NUL.
        let text = text.replace('\0', "");
        if let Err(e) = self.0.set_clipboard_text(&text) {
            log::debug!("clipboard: write failed: {e}");
        }
    }
}

impl servo::ClipboardDelegate for Clipboard {
    fn clear(&self, _webview: servo::WebView) {
        self.set_text("");
    }

    fn get_text(&self, _webview: servo::WebView, request: servo::StringRequest) {
        request.success(self.text());
    }

    fn set_text(&self, _webview: servo::WebView, new_contents: String) {
        Clipboard::set_text(self, &new_contents);
    }
}
