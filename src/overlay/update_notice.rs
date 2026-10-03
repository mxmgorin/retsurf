//! The startup notice of a newer build: a modal card offering Update or Later.

/// The card's two buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Update,
    Later,
}

/// The notice's state; shown while it holds a version.
#[derive(Default)]
pub struct UpdateNotice {
    version: Option<String>,
    later: bool,
}

impl UpdateNotice {
    /// Raise the notice for `version`, focused on Update.
    pub fn show(&mut self, version: String) {
        self.version = Some(version);
        self.later = false;
    }

    pub fn close(&mut self) {
        self.version = None;
    }

    pub fn visible(&self) -> bool {
        self.version.is_some()
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub fn selected(&self) -> Choice {
        if self.later {
            Choice::Later
        } else {
            Choice::Update
        }
    }

    /// Left picks Update, Right picks Later.
    pub fn move_sel(&mut self, dx: i32) {
        if dx != 0 {
            self.later = dx > 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_on_update_and_steps_to_later() {
        let mut notice = UpdateNotice::default();
        notice.move_sel(1);
        notice.show("0.9.1".into());
        assert_eq!(notice.selected(), Choice::Update);
        notice.move_sel(1);
        assert_eq!(notice.selected(), Choice::Later);
        notice.move_sel(-1);
        assert_eq!(notice.selected(), Choice::Update);
    }
}
