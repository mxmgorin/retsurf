//! The startup notice of a newer build: a modal card that offers the update and
//! follows its install through to the quit that applies it.

use crate::update::{Offer, UpdateState};

/// A button on the card.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Choice {
    /// Download and swap the release in place.
    #[default]
    Install,
    /// Open the release page to download by hand.
    Open,
    /// Check again after a failure.
    Retry,
    /// Quit so the swapped build runs next.
    Quit,
    /// Close the card, the offer still standing.
    Later,
    /// Close the card while the install runs on.
    Hide,
}

/// The card's buttons for `state`, left to right; never empty.
pub fn choices(state: &UpdateState) -> &'static [Choice] {
    match state {
        UpdateState::Available {
            offer: Offer::Install { .. },
            ..
        } => &[Choice::Install, Choice::Later],
        UpdateState::Available {
            offer: Offer::Open,
            page: Some(_),
            ..
        } => &[Choice::Open, Choice::Later],
        UpdateState::Installed { .. } => &[Choice::Quit, Choice::Later],
        UpdateState::Error(_) => &[Choice::Retry, Choice::Later],
        UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Installing => {
            &[Choice::Hide]
        }
        UpdateState::Available {
            offer: Offer::Open,
            page: None,
            ..
        }
        | UpdateState::Idle
        | UpdateState::UpToDate { .. } => &[Choice::Later],
    }
}

/// The notice's state; shown while it holds a version.
#[derive(Default)]
pub struct UpdateNotice {
    version: Option<String>,
    focus: Choice,
}

impl UpdateNotice {
    /// Raise the notice for `version`, focused on its first button.
    pub fn show(&mut self, version: String) {
        self.version = Some(version);
        self.focus = Choice::default();
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

    /// The focused button for `state`: the last one focused while it is shown,
    /// else the first.
    pub fn selected(&self, state: &UpdateState) -> Choice {
        let choices = choices(state);
        match choices.contains(&self.focus) {
            true => self.focus,
            false => choices[0],
        }
    }

    pub fn focus(&mut self, choice: Choice) {
        self.focus = choice;
    }

    /// Left focuses the first button of `state`, Right the last.
    pub fn move_sel(&mut self, dx: i32, state: &UpdateState) {
        let choices = choices(state);
        let target = match dx.signum() {
            -1 => choices.first(),
            1 => choices.last(),
            _ => None,
        };
        if let Some(&choice) = target {
            self.focus = choice;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> UpdateState {
        UpdateState::Available {
            version: "0.11.0".into(),
            notes: None,
            page: None,
            offer: Offer::Install {
                url: String::new(),
                size: 0,
                sha256: None,
            },
        }
    }

    #[test]
    fn opens_on_install_and_steps_to_later() {
        let mut notice = UpdateNotice::default();
        notice.move_sel(1, &offer());
        notice.show("0.11.0".into());
        assert_eq!(notice.selected(&offer()), Choice::Install);
        notice.move_sel(1, &offer());
        assert_eq!(notice.selected(&offer()), Choice::Later);
        notice.move_sel(-1, &offer());
        assert_eq!(notice.selected(&offer()), Choice::Install);
    }

    #[test]
    fn a_new_button_set_focuses_its_first() {
        let mut notice = UpdateNotice::default();
        notice.show("0.11.0".into());
        let downloading = UpdateState::Downloading {
            received: 0,
            total: 1,
        };
        assert_eq!(notice.selected(&downloading), Choice::Hide);
        notice.move_sel(1, &downloading);
        let installed = UpdateState::Installed {
            version: "0.11.0".into(),
        };
        assert_eq!(notice.selected(&installed), Choice::Quit);
    }
}
