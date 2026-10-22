//! Freedesktop desktop entries: the start-at-login file written to
//! `~/.config/autostart` on Linux.
//!
//! An `Exec` argument goes through two escaping layers. The Exec layer
//! quotes the path, backslash-escapes `"`, `` ` ``, `$` and `\` inside the
//! quotes and doubles `%` so it is not read as a field code. The value is
//! then a desktop-entry string, whose own escaping turns each `\` into `\\`
//! (and newline, tab and carriage return into `\n`, `\t`, `\r`, so a path
//! cannot end the line and add keys).

/// The `Exec` value that runs `path` with no arguments, escaped for both
/// layers, ready to follow `Exec=`.
pub fn exec_value(path: &str) -> String {
    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('"');
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    string_value(&quoted)
}

/// `value` escaped as a desktop-entry string.
fn string_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// The autostart entry that starts `path` at login, named `name`. The keys
/// other than `Exec` match what earlier releases wrote through the
/// autostart plugin.
pub fn autostart_entry(name: &str, path: &str) -> String {
    let name = string_value(name);
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name={name}\n\
         Comment={name} startup script\n\
         Exec={}\n\
         StartupNotify=false\n\
         Terminal=false\n",
        exec_value(path)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_path_is_quoted() {
        assert_eq!(
            exec_value("/usr/bin/usage-monitor"),
            "\"/usr/bin/usage-monitor\""
        );
    }

    #[test]
    fn path_with_a_space_stays_one_argument() {
        assert_eq!(
            exec_value("/home/me/My Apps/usage-monitor.AppImage"),
            "\"/home/me/My Apps/usage-monitor.AppImage\""
        );
    }

    #[test]
    fn dollar_is_escaped_for_exec_then_for_the_string() {
        // Exec layer: `\$`; string layer doubles the backslash.
        assert_eq!(exec_value("/opt/a$b/app"), "\"/opt/a\\\\$b/app\"");
    }

    #[test]
    fn quote_backtick_and_backslash_are_escaped() {
        assert_eq!(exec_value("/a\"b"), "\"/a\\\\\"b\"");
        assert_eq!(exec_value("/a`b"), "\"/a\\\\`b\"");
        // `\` becomes `\\` in the Exec layer, then `\\\\` as a string.
        assert_eq!(exec_value("/a\\b"), "\"/a\\\\\\\\b\"");
    }

    #[test]
    fn percent_is_not_a_field_code() {
        assert_eq!(exec_value("/opt/100%/app"), "\"/opt/100%%/app\"");
    }

    #[test]
    fn newline_cannot_start_a_new_key() {
        let entry = autostart_entry("UsageMonitor", "/tmp/x\nHidden=true/app");
        assert!(!entry.lines().any(|line| line == "Hidden=true/app\""));
        assert!(entry.contains("Exec=\"/tmp/x\\nHidden=true/app\"\n"));
    }

    #[test]
    fn entry_keeps_the_keys_earlier_releases_wrote() {
        let entry = autostart_entry("UsageMonitor", "/home/me/My Apps/u.AppImage");
        assert_eq!(
            entry,
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.0\n\
             Name=UsageMonitor\n\
             Comment=UsageMonitor startup script\n\
             Exec=\"/home/me/My Apps/u.AppImage\"\n\
             StartupNotify=false\n\
             Terminal=false\n"
        );
    }
}
