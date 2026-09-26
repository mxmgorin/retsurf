//! Selection and clipboard edits on the field the keyboard types into. The
//! chrome's own fields keep the selection here, as an anchor beside the caret;
//! a page field keeps its own, so the page gets the keys a desktop would send.

use super::{byte_at, caret_field, Osk, OskTarget};
use crate::browser::AppBrowser;
use crate::event::sdl2_servo::{code_for_char, key_event};
use keyboard_types::{Code, Key, Modifiers, NamedKey};

/// A selection or clipboard edit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edit {
    SelectAll,
    Copy,
    Cut,
    Paste,
    /// Grow or shrink the selection by one char, `dx` being -1 or 1.
    Extend(i32),
}

impl Osk {
    /// Apply `edit` to `target`; a field without a drawn caret has no selection.
    pub(super) fn edit(&mut self, edit: Edit, target: OskTarget, browser: &AppBrowser) {
        if matches!(target, OskTarget::Page) {
            page_edit(edit, browser);
            return;
        }
        if !caret_field(&target) {
            return;
        }
        let clipboard = browser.clipboard();
        let mut state = browser.get_state_mut();
        let buf = match target {
            OskTarget::AddressBar => &mut state.location,
            OskTarget::Prompt(buf) | OskTarget::Home(buf) | OskTarget::DialEdit(buf) => buf,
            _ => unreachable!("caret_field admits only the address bar and buffer fields"),
        };
        let len = buf.chars().count();
        match edit {
            Edit::SelectAll => {
                self.anchor = Some(0);
                self.caret = len;
            }
            Edit::Extend(dx) => {
                let caret = self.caret.min(len);
                self.anchor.get_or_insert(caret);
                self.caret = caret.saturating_add_signed(dx as isize).min(len);
            }
            Edit::Copy => {
                if let Some(range) = self.selection() {
                    clipboard.set_text(slice(buf, range));
                }
            }
            Edit::Cut => {
                if let Some(range) = self.selection() {
                    clipboard.set_text(slice(buf, range));
                    self.delete_selection(buf);
                }
            }
            Edit::Paste => {
                // A single-line field: a pasted newline would end up invisible.
                let text = clipboard.text().replace(['\r', '\n'], " ");
                self.delete_selection(buf);
                let caret = self.caret.min(buf.chars().count());
                buf.insert_str(byte_at(buf, caret), &text);
                self.caret = caret + text.chars().count();
            }
        }
    }

    /// The selected char range, start first; `None` when nothing is selected.
    pub(super) fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor.filter(|a| *a != self.caret)?;
        Some((anchor.min(self.caret), anchor.max(self.caret)))
    }

    /// Delete the selection from `buf`, leaving the caret where it began.
    /// Returns whether there was one.
    pub(super) fn delete_selection(&mut self, buf: &mut String) -> bool {
        let range = self.selection();
        self.anchor = None;
        let Some((start, end)) = range else {
            return false;
        };
        let len = buf.chars().count();
        let (start, end) = (start.min(len), end.min(len));
        buf.replace_range(byte_at(buf, start)..byte_at(buf, end), "");
        self.caret = start;
        true
    }
}

/// The chars of `buf` in `range`, clamped to its length.
fn slice(buf: &str, (start, end): (usize, usize)) -> &str {
    &buf[byte_at(buf, start)..byte_at(buf, end)]
}

/// A page field edits itself; send what a desktop keyboard would.
fn page_edit(edit: Edit, browser: &AppBrowser) {
    let (key, code, modifiers) = match edit {
        Edit::SelectAll => ctrl('a'),
        Edit::Copy => ctrl('c'),
        Edit::Cut => ctrl('x'),
        Edit::Paste => ctrl('v'),
        Edit::Extend(dx) if dx < 0 => (
            Key::Named(NamedKey::ArrowLeft),
            Code::ArrowLeft,
            Modifiers::SHIFT,
        ),
        Edit::Extend(_) => (
            Key::Named(NamedKey::ArrowRight),
            Code::ArrowRight,
            Modifiers::SHIFT,
        ),
    };
    for down in [true, false] {
        let event = key_event(key.clone(), code, modifiers, down);
        browser.handle_input(servo::InputEvent::Keyboard(event));
    }
}

fn ctrl(c: char) -> (Key, Code, Modifiers) {
    (
        Key::Character(c.to_string()),
        code_for_char(c),
        Modifiers::CONTROL,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OskConfig, PadLayout};

    fn osk() -> Osk {
        Osk::new(&OskConfig::default(), PadLayout::default())
    }

    #[test]
    fn a_backward_selection_reads_start_first() {
        let mut osk = osk();
        osk.caret = 1;
        osk.anchor = Some(4);
        assert_eq!(osk.selection(), Some((1, 4)));
        osk.anchor = Some(1);
        assert_eq!(osk.selection(), None);
    }

    #[test]
    fn deleting_a_selection_leaves_the_caret_at_its_start() {
        let mut osk = osk();
        let mut buf = "héllo world".to_string();
        osk.caret = 5;
        osk.anchor = Some(1);
        assert!(osk.delete_selection(&mut buf));
        assert_eq!(buf, "h world");
        assert_eq!((osk.caret, osk.anchor), (1, None));
        assert!(!osk.delete_selection(&mut buf));
    }

    /// Text edited from outside can shrink under a held selection.
    #[test]
    fn a_selection_past_the_end_is_clamped() {
        let mut osk = osk();
        let mut buf = "abc".to_string();
        osk.caret = 9;
        osk.anchor = Some(1);
        assert!(osk.delete_selection(&mut buf));
        assert_eq!(buf, "a");
    }
}
