//! Bubble visibility: the user's show/hide choice and the fullscreen
//! auto-hide, combined into show and hide actions.
//!
//! A user show wins over fullscreen and shows at once; leaving fullscreen
//! shows the bubbles again only when the user has not hidden them.

/// The two reasons a bubble can be hidden.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Visibility {
    pub user_hidden: bool,
    pub hidden_by_fullscreen: bool,
}

impl Visibility {
    /// The state at start-up, from the saved `widget_visible` setting.
    pub fn from_widget_visible(widget_visible: bool) -> Self {
        Self {
            user_hidden: !widget_visible,
            hidden_by_fullscreen: false,
        }
    }

    /// Whether a bubble created now should be shown.
    pub fn shows(&self) -> bool {
        !self.user_hidden && !self.hidden_by_fullscreen
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    UserShow,
    UserHide,
    FullscreenOn,
    FullscreenOff,
}

/// What to do with every bubble after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Show,
    Hide,
}

/// Apply `event` to `state`. Returns the new state and the action for the
/// bubbles, `None` when they stay as they are.
pub fn next(state: Visibility, event: Event) -> (Visibility, Option<Action>) {
    match event {
        Event::UserShow => (
            Visibility {
                user_hidden: false,
                ..state
            },
            Some(Action::Show),
        ),
        Event::UserHide => (
            Visibility {
                user_hidden: true,
                ..state
            },
            Some(Action::Hide),
        ),
        Event::FullscreenOn if state.hidden_by_fullscreen => (state, None),
        Event::FullscreenOn => (
            Visibility {
                hidden_by_fullscreen: true,
                ..state
            },
            Some(Action::Hide),
        ),
        Event::FullscreenOff if !state.hidden_by_fullscreen => (state, None),
        Event::FullscreenOff => (
            Visibility {
                hidden_by_fullscreen: false,
                ..state
            },
            (!state.user_hidden).then_some(Action::Show),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: Visibility = Visibility {
        user_hidden: false,
        hidden_by_fullscreen: false,
    };

    fn state(user_hidden: bool, hidden_by_fullscreen: bool) -> Visibility {
        Visibility {
            user_hidden,
            hidden_by_fullscreen,
        }
    }

    #[test]
    fn user_show_shows_at_once_even_during_fullscreen() {
        assert_eq!(
            next(state(true, false), Event::UserShow),
            (SHOWN, Some(Action::Show))
        );
        assert_eq!(
            next(state(true, true), Event::UserShow),
            (state(false, true), Some(Action::Show))
        );
    }

    #[test]
    fn user_hide_hides() {
        assert_eq!(
            next(SHOWN, Event::UserHide),
            (state(true, false), Some(Action::Hide))
        );
        assert_eq!(
            next(state(false, true), Event::UserHide),
            (state(true, true), Some(Action::Hide))
        );
    }

    #[test]
    fn fullscreen_on_hides_once() {
        assert_eq!(
            next(SHOWN, Event::FullscreenOn),
            (state(false, true), Some(Action::Hide))
        );
        assert_eq!(
            next(state(true, false), Event::FullscreenOn),
            (state(true, true), Some(Action::Hide))
        );
        assert_eq!(
            next(state(false, true), Event::FullscreenOn),
            (state(false, true), None)
        );
    }

    #[test]
    fn fullscreen_off_shows_only_when_the_user_has_not_hidden() {
        assert_eq!(
            next(state(false, true), Event::FullscreenOff),
            (SHOWN, Some(Action::Show))
        );
        assert_eq!(
            next(state(true, true), Event::FullscreenOff),
            (state(true, false), None)
        );
        assert_eq!(next(SHOWN, Event::FullscreenOff), (SHOWN, None));
    }

    #[test]
    fn start_up_state_follows_the_saved_setting() {
        assert!(Visibility::from_widget_visible(true).shows());
        assert!(!Visibility::from_widget_visible(false).shows());
        assert!(!state(false, true).shows());
    }
}
