//! Slash-command parsing for the input line.
//!
//! Pure: given a line of text, decide what it means. No I/O, no rendering.

/// A parsed input line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Empty input; do nothing.
    Empty,
    /// A normal chat message to send to every peer with a session.
    Message(String),
    Help,
    Quit,
    Clear,
    Nick(String),
    Who,
    Status,
    Fingerprint,
    Me(String),
    /// Open the interactive settings customizer, optionally on one setting.
    Settings(String),
    /// Switch theme; empty argument lists the presets.
    Theme(String),
    /// Toggle timestamps for this session.
    Timestamps,
    /// List the active key bindings.
    Keys,
    /// Show the config file path and current theme.
    Config,
    /// An unrecognised `/command`.
    Unknown(String),
}

/// Every command name, for tab completion and help output.
pub const COMMANDS: &[&str] = &[
    "/help",
    "/quit",
    "/exit",
    "/clear",
    "/nick",
    "/who",
    "/status",
    "/fingerprint",
    "/me",
    "/settings",
    "/theme",
    "/timestamps",
    "/keys",
    "/config",
];

/// Parse one input line into a [`Command`].
pub fn parse(input: &str) -> Command {
    let text = input.trim();
    if text.is_empty() {
        return Command::Empty;
    }
    let Some(rest) = text.strip_prefix('/') else {
        return Command::Message(text.to_string());
    };
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    match name {
        "help" => Command::Help,
        "quit" | "exit" => Command::Quit,
        "clear" => Command::Clear,
        "nick" => Command::Nick(arg.to_string()),
        "who" => Command::Who,
        "status" => Command::Status,
        "fingerprint" => Command::Fingerprint,
        "me" => Command::Me(arg.to_string()),
        "settings" | "customize" => Command::Settings(arg.to_string()),
        "theme" => Command::Theme(arg.to_string()),
        "timestamps" => Command::Timestamps,
        "keys" => Command::Keys,
        "config" => Command::Config,
        other => Command::Unknown(other.to_string()),
    }
}

/// Command names that start with `prefix`, for tab completion.
pub fn matching(prefix: &str) -> Vec<&'static str> {
    COMMANDS
        .iter()
        .copied()
        .filter(|name| name.starts_with(prefix))
        .collect()
}

/// Human-readable help, one line per command.
pub fn help_lines() -> Vec<String> {
    vec![
        "/help          show this help".to_string(),
        "/quit, /exit   save state and exit".to_string(),
        "/clear         clear the scrollback".to_string(),
        "/nick <name>   set and broadcast your nickname".to_string(),
        "/who           list peers and their session state".to_string(),
        "/status        show connection and room details".to_string(),
        "/fingerprint   print the room fingerprint".to_string(),
        "/me <action>   send an emote".to_string(),
        "/settings [k]  open the customizer, optionally on setting k (F2)".to_string(),
        "/theme [name]  list or switch themes for this session (F4)".to_string(),
        "/timestamps    toggle timestamps for this session (F3)".to_string(),
        "/keys          list the active key bindings".to_string(),
        "/config        show the config file path and theme".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nick() {
        assert_eq!(parse("/nick bob"), Command::Nick("bob".to_string()));
        assert_eq!(parse("  /nick  bob  "), Command::Nick("bob".to_string()));
    }

    #[test]
    fn parses_me() {
        assert_eq!(parse("/me waves"), Command::Me("waves".to_string()));
        assert_eq!(parse("/me"), Command::Me(String::new()));
    }

    #[test]
    fn parses_unknown() {
        assert_eq!(parse("/unknown"), Command::Unknown("unknown".to_string()));
        assert_eq!(parse("/"), Command::Unknown(String::new()));
    }

    #[test]
    fn parses_empty() {
        assert_eq!(parse(""), Command::Empty);
        assert_eq!(parse("   "), Command::Empty);
    }

    #[test]
    fn parses_plain_message() {
        assert_eq!(
            parse("hello world"),
            Command::Message("hello world".to_string())
        );
    }

    #[test]
    fn parses_simple_commands() {
        assert_eq!(parse("/help"), Command::Help);
        assert_eq!(parse("/quit"), Command::Quit);
        assert_eq!(parse("/exit"), Command::Quit);
        assert_eq!(parse("/clear"), Command::Clear);
        assert_eq!(parse("/who"), Command::Who);
        assert_eq!(parse("/status"), Command::Status);
        assert_eq!(parse("/fingerprint"), Command::Fingerprint);
        assert_eq!(parse("/settings"), Command::Settings(String::new()));
        assert_eq!(parse("/customize"), Command::Settings(String::new()));
        assert_eq!(
            parse("/settings theme.preset"),
            Command::Settings("theme.preset".to_string())
        );
        assert_eq!(parse("/theme"), Command::Theme(String::new()));
        assert_eq!(parse("/theme nord"), Command::Theme("nord".to_string()));
        assert_eq!(parse("/timestamps"), Command::Timestamps);
        assert_eq!(parse("/keys"), Command::Keys);
        assert_eq!(parse("/config"), Command::Config);
    }

    #[test]
    fn completion_matches_prefix() {
        assert_eq!(matching("/ni"), vec!["/nick"]);
        assert_eq!(matching("/"), COMMANDS.to_vec());
        assert!(matching("/zzz").is_empty());
    }

    #[test]
    fn help_lists_every_command() {
        let help = help_lines().join("\n");
        for command in COMMANDS {
            // `/exit` is an alias documented next to `/quit`.
            if *command == "/exit" {
                continue;
            }
            assert!(help.contains(command), "help is missing {command}");
        }
    }
}
