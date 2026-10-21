//! The context menu as a plain tree with string ids, independent of any
//! menu toolkit. The tray and bubble menus are built from it, and menu
//! events come back as [`MenuCommand::parse`] of the clicked id.

use crate::core::geom::{DEFAULT_BUBBLE_SIZE, MAX_BUBBLE_SIZE, MIN_BUBBLE_SIZE};
use crate::core::i18n::LocaleStrings;
use crate::core::settings::{Settings, POLL_INTERVALS, UPDATE_CHECK_INTERVALS};
use crate::core::state::{version_action_label, UpdateStatus};
use crate::usage::ProviderId;

/// Every action a menu item can trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    Refresh,
    PollInterval(u32),
    ToggleProvider(ProviderId),
    Autostart,
    ResetPosition,
    VersionAction,
    /// `None` disables the automatic check.
    UpdateInterval(Option<u64>),
    SizeSmaller,
    SizeLarger,
    SizeReset,
    ToggleWidget,
    Restart,
    Exit,
}

impl MenuCommand {
    /// The item id: `refresh`, `freq:<ms>`, `provider:<slug>`, `autostart`,
    /// `reset-position`, `version`, `update-interval:<secs|off>`,
    /// `size-smaller`, `size-larger`, `size-reset`, `toggle-widget`,
    /// `restart` or `exit`.
    pub fn id(self) -> String {
        match self {
            MenuCommand::Refresh => "refresh".into(),
            MenuCommand::PollInterval(ms) => format!("freq:{ms}"),
            MenuCommand::ToggleProvider(p) => format!("provider:{}", p.slug()),
            MenuCommand::Autostart => "autostart".into(),
            MenuCommand::ResetPosition => "reset-position".into(),
            MenuCommand::VersionAction => "version".into(),
            MenuCommand::UpdateInterval(None) => "update-interval:off".into(),
            MenuCommand::UpdateInterval(Some(secs)) => format!("update-interval:{secs}"),
            MenuCommand::SizeSmaller => "size-smaller".into(),
            MenuCommand::SizeLarger => "size-larger".into(),
            MenuCommand::SizeReset => "size-reset".into(),
            MenuCommand::ToggleWidget => "toggle-widget".into(),
            MenuCommand::Restart => "restart".into(),
            MenuCommand::Exit => "exit".into(),
        }
    }

    /// Parse an item id. Only values the menu offers are accepted, so an
    /// unknown or hand-made id maps to `None` instead of an arbitrary
    /// interval.
    pub fn parse(id: &str) -> Option<Self> {
        let simple = match id {
            "refresh" => Some(MenuCommand::Refresh),
            "autostart" => Some(MenuCommand::Autostart),
            "reset-position" => Some(MenuCommand::ResetPosition),
            "version" => Some(MenuCommand::VersionAction),
            "size-smaller" => Some(MenuCommand::SizeSmaller),
            "size-larger" => Some(MenuCommand::SizeLarger),
            "size-reset" => Some(MenuCommand::SizeReset),
            "toggle-widget" => Some(MenuCommand::ToggleWidget),
            "restart" => Some(MenuCommand::Restart),
            "exit" => Some(MenuCommand::Exit),
            _ => None,
        };
        if simple.is_some() {
            return simple;
        }
        let (kind, value) = id.split_once(':')?;
        match kind {
            "freq" => {
                let ms = value.parse::<u32>().ok()?;
                POLL_INTERVALS
                    .contains(&ms)
                    .then_some(MenuCommand::PollInterval(ms))
            }
            "provider" => ProviderId::from_slug(value).map(MenuCommand::ToggleProvider),
            "update-interval" => {
                let secs = if value == "off" {
                    None
                } else {
                    Some(value.parse::<u64>().ok()?)
                };
                UPDATE_CHECK_INTERVALS
                    .contains(&secs)
                    .then_some(MenuCommand::UpdateInterval(secs))
            }
            _ => None,
        }
    }
}

/// One node of the menu tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Action {
        command: MenuCommand,
        label: String,
        enabled: bool,
    },
    Check {
        command: MenuCommand,
        label: String,
        checked: bool,
        enabled: bool,
    },
    /// A greyed, id-less hint row.
    Hint {
        label: String,
    },
    Separator,
    Submenu {
        label: String,
        entries: Vec<MenuEntry>,
    },
}

/// Everything the menu reflects, snapshotted outside the state lock.
#[derive(Clone, Copy, Debug)]
pub struct MenuInput<'a> {
    pub settings: &'a Settings,
    pub strings: &'a LocaleStrings,
    pub autostart_enabled: bool,
    pub update_status: UpdateStatus,
}

fn action(command: MenuCommand, label: &str, enabled: bool) -> MenuEntry {
    MenuEntry::Action {
        command,
        label: label.to_string(),
        enabled,
    }
}

fn check(command: MenuCommand, label: &str, checked: bool, enabled: bool) -> MenuEntry {
    MenuEntry::Check {
        command,
        label: label.to_string(),
        checked,
        enabled,
    }
}

fn hint(label: &str) -> MenuEntry {
    MenuEntry::Hint {
        label: label.to_string(),
    }
}

fn submenu(label: &str, entries: Vec<MenuEntry>) -> MenuEntry {
    MenuEntry::Submenu {
        label: label.to_string(),
        entries,
    }
}

/// Build the context menu in display order.
pub fn build(input: MenuInput<'_>) -> Vec<MenuEntry> {
    let MenuInput {
        settings,
        strings: s,
        autostart_enabled,
        update_status,
    } = input;

    let freq_labels = [
        &s.one_minute,
        &s.five_minutes,
        &s.fifteen_minutes,
        &s.one_hour,
    ];
    let frequency = POLL_INTERVALS
        .iter()
        .zip(freq_labels)
        .map(|(&ms, label)| {
            check(
                MenuCommand::PollInterval(ms),
                label,
                settings.poll_interval_ms == ms,
                true,
            )
        })
        .collect();

    let providers = ProviderId::ALL
        .iter()
        .map(|&p| {
            check(
                MenuCommand::ToggleProvider(p),
                s.provider_label(p),
                settings.is_provider_enabled(p),
                p.metadata().live_usage,
            )
        })
        .collect();

    let auto_labels = [
        &s.auto_check_disabled,
        &s.auto_check_hourly,
        &s.auto_check_daily,
        &s.auto_check_weekly,
    ];
    let auto_update = UPDATE_CHECK_INTERVALS
        .iter()
        .zip(auto_labels)
        .map(|(&secs, label)| {
            check(
                MenuCommand::UpdateInterval(secs),
                label,
                settings.update_check_interval_secs == secs,
                true,
            )
        })
        .collect();

    let size = settings.bubble_size_logical;
    let settings_menu = vec![
        check(
            MenuCommand::Autostart,
            s.autostart_label(),
            autostart_enabled,
            true,
        ),
        action(MenuCommand::ResetPosition, &s.reset_position, true),
        action(
            MenuCommand::VersionAction,
            &version_action_label(update_status, s),
            !update_status.is_busy(),
        ),
        submenu(&s.auto_update_check, auto_update),
    ];

    let controls = vec![
        action(
            MenuCommand::SizeSmaller,
            &s.size_smaller,
            size > MIN_BUBBLE_SIZE,
        ),
        action(
            MenuCommand::SizeLarger,
            &s.size_larger,
            size < MAX_BUBBLE_SIZE,
        ),
        action(
            MenuCommand::SizeReset,
            &s.reset_size,
            size != DEFAULT_BUBBLE_SIZE,
        ),
        MenuEntry::Separator,
        hint(&s.control_left_click),
        hint(&s.control_right_click),
        hint(&s.control_drag),
        hint(&s.control_ctrl_wheel),
        hint(&s.control_tray_click),
    ];

    vec![
        action(MenuCommand::Refresh, &s.refresh, true),
        submenu(&s.update_frequency, frequency),
        submenu(&s.providers, providers),
        submenu(&s.settings, settings_menu),
        submenu(&s.controls, controls),
        check(
            MenuCommand::ToggleWidget,
            &s.show_widget,
            settings.widget_visible,
            true,
        ),
        MenuEntry::Separator,
        action(MenuCommand::Restart, &s.restart, true),
        action(MenuCommand::Exit, &s.exit, true),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::i18n::I18n;
    use crate::core::settings::{POLL_15_MIN, UPDATE_CHECK_DAILY_SECS};

    fn menu(settings: &Settings, status: UpdateStatus) -> Vec<MenuEntry> {
        let strings = I18n::load().strings().clone();
        build(MenuInput {
            settings,
            strings: &strings,
            autostart_enabled: true,
            update_status: status,
        })
    }

    fn find<'a>(entries: &'a [MenuEntry], id: &str) -> Option<&'a MenuEntry> {
        entries.iter().find_map(|e| match e {
            MenuEntry::Action { command, .. } | MenuEntry::Check { command, .. }
                if command.id() == id =>
            {
                Some(e)
            }
            MenuEntry::Submenu { entries, .. } => find(entries, id),
            _ => None,
        })
    }

    fn enabled(entries: &[MenuEntry], id: &str) -> bool {
        match find(entries, id) {
            Some(MenuEntry::Action { enabled, .. } | MenuEntry::Check { enabled, .. }) => *enabled,
            other => panic!("no item {id}: {other:?}"),
        }
    }

    fn checked(entries: &[MenuEntry], id: &str) -> bool {
        match find(entries, id) {
            Some(MenuEntry::Check { checked, .. }) => *checked,
            other => panic!("no check item {id}: {other:?}"),
        }
    }

    fn with_size(size: i32) -> Settings {
        Settings {
            bubble_size_logical: size,
            ..Settings::default()
        }
    }

    #[test]
    fn size_items_grey_out_at_the_limits() {
        let m = menu(&with_size(140), UpdateStatus::Idle);
        assert!(!enabled(&m, "size-smaller"));
        assert!(enabled(&m, "size-larger"));
        assert!(enabled(&m, "size-reset"));
        let m = menu(&with_size(360), UpdateStatus::Idle);
        assert!(enabled(&m, "size-smaller"));
        assert!(!enabled(&m, "size-larger"));
        assert!(enabled(&m, "size-reset"));
        let m = menu(&with_size(200), UpdateStatus::Idle);
        assert!(enabled(&m, "size-smaller"));
        assert!(enabled(&m, "size-larger"));
        assert!(!enabled(&m, "size-reset"));
    }

    #[test]
    fn checks_follow_settings() {
        let settings = Settings {
            poll_interval_ms: POLL_15_MIN,
            update_check_interval_secs: Some(UPDATE_CHECK_DAILY_SECS),
            show_codex: true,
            show_opencode_go: true,
            widget_visible: false,
            ..Settings::default()
        };
        let m = menu(&settings, UpdateStatus::Idle);
        assert!(checked(&m, "freq:900000"));
        assert!(!checked(&m, "freq:300000"));
        assert!(checked(&m, "update-interval:86400"));
        assert!(!checked(&m, "update-interval:off"));
        assert!(checked(&m, "provider:claude"));
        assert!(checked(&m, "provider:chatgpt"));
        // OpenCode Go mirrors its flag but stays greyed.
        assert!(checked(&m, "provider:opencode-go"));
        assert!(!enabled(&m, "provider:opencode-go"));
        assert!(checked(&m, "autostart"));
        assert!(!checked(&m, "toggle-widget"));
    }

    #[test]
    fn version_item_is_disabled_while_busy() {
        assert!(!enabled(
            &menu(&Settings::default(), UpdateStatus::Checking),
            "version"
        ));
        assert!(!enabled(
            &menu(&Settings::default(), UpdateStatus::Applying),
            "version"
        ));
        assert!(enabled(
            &menu(&Settings::default(), UpdateStatus::Available),
            "version"
        ));
    }

    #[test]
    fn top_level_order_and_hint_rows() {
        let m = menu(&Settings::default(), UpdateStatus::Idle);
        let labels: Vec<String> = m
            .iter()
            .map(|e| match e {
                MenuEntry::Action { label, .. }
                | MenuEntry::Check { label, .. }
                | MenuEntry::Hint { label }
                | MenuEntry::Submenu { label, .. } => label.clone(),
                MenuEntry::Separator => "-".into(),
            })
            .collect();
        assert_eq!(
            labels,
            [
                "Refresh",
                "Update frequency",
                "Providers",
                "Settings",
                "Controls",
                "Show widget",
                "-",
                "Restart",
                "Exit"
            ]
        );
        let Some(MenuEntry::Submenu { entries, .. }) = m.get(4) else {
            panic!("controls submenu missing");
        };
        assert_eq!(entries.len(), 9);
        assert_eq!(entries[3], MenuEntry::Separator);
        assert!(entries[4..]
            .iter()
            .all(|e| matches!(e, MenuEntry::Hint { .. })));
    }

    #[test]
    fn ids_round_trip_and_unknown_ids_are_rejected() {
        let m = menu(&Settings::default(), UpdateStatus::Idle);
        fn walk(entries: &[MenuEntry], out: &mut Vec<MenuCommand>) {
            for e in entries {
                match e {
                    MenuEntry::Action { command, .. } | MenuEntry::Check { command, .. } => {
                        out.push(*command)
                    }
                    MenuEntry::Submenu { entries, .. } => walk(entries, out),
                    _ => {}
                }
            }
        }
        let mut commands = Vec::new();
        walk(&m, &mut commands);
        assert_eq!(commands.len(), 21);
        for c in commands {
            assert_eq!(MenuCommand::parse(&c.id()), Some(c), "{}", c.id());
        }
        for bad in [
            "",
            "freq:1",
            "freq:abc",
            "provider:codex",
            "update-interval:5",
            "nope:1",
        ] {
            assert_eq!(MenuCommand::parse(bad), None, "{bad}");
        }
    }
}
