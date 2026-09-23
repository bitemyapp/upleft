//! Port of `Tests/DownrightAppTests/WelcomeTourTests.swift`.

use upleft_app::support::commands::{Command, KeyBinding, ModifierFlags};
use upleft_app::support::keybindings::KeybindingDefaults;
use upleft_app::support::welcome_tour::{WelcomeTour, WelcomeTourError};
use upleft_swift_text as swift_text;

fn source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/downright/Resources/Welcome.md");
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn every_tour_shortcut_token_names_a_command_in_the_command_table() {
    let source = source();
    let tokens = WelcomeTour::tokens(&source);
    assert!(!tokens.is_empty());
    for token in &tokens {
        let parts = swift_text::split(token, ':', 1, true);
        assert_eq!(parts.len(), 2, "malformed tour token: {token}");
        if parts.len() != 2 {
            continue;
        }
        let command = Command::from_raw_value(parts[1]).unwrap_or_else(|| panic!("{} is not a command", parts[1]));
        if parts[0] == "shortcut" {
            // A user may intentionally clear this binding; that must not
            // make the tour fail to materialize.
            let _ = KeybindingDefaults::table().get(&command);
        } else {
            assert_eq!(parts[0], "command", "unknown tour token kind: {token}");
        }
    }
    let rendered =
        WelcomeTour::render(&source, |command| KeybindingDefaults::table().get(&command).and_then(|b| b.first().cloned()))
            .unwrap();
    assert!(!rendered.contains("{{"));
    let unbound = WelcomeTour::render(&source, |_| None).unwrap();
    assert!(unbound.contains("Unassigned"));
}

#[test]
fn rendered_tour_uses_customized_bindings() {
    let rendered = WelcomeTour::render("Use {{shortcut:documentLens}} to open {{command:documentLens}}.", |command| {
        (command == Command::DocumentLens)
            .then(|| KeyBinding::new("o", ModifierFlags::COMMAND.union(ModifierFlags::SHIFT)))
    })
    .unwrap();
    assert_eq!(rendered, "Use ⇧⌘O to open Contents / Outline.");
}

#[test]
fn invalid_tour_tokens_fail_closed() {
    assert_eq!(
        WelcomeTour::render("{{shortcut:missing}}", |_| None),
        Err(WelcomeTourError::UnknownCommand("missing".into()))
    );
    assert_eq!(WelcomeTour::render("{{shortcut:documentLens}}", |_| None), Ok("Unassigned".into()));
    assert_eq!(
        WelcomeTour::render("{{shortcut:documentLens", |_| None),
        Err(WelcomeTourError::MalformedToken("{{shortcut:documentLens".into()))
    );
}
