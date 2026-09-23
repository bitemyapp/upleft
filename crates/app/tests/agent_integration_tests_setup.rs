//! Port of the "Setup panel" tests of
//! `Tests/DownrightAppTests/AgentIntegrationTests.swift`
//! (`agentStepIsNotPreselected`, `everyStepExplainsItself`,
//! `agentStepNamesTheFileItEdits`). They check `SetupWindowController.Step`
//! (`app::setup_window_controller::Step`), a plain value type, so no window
//! is built and nothing reads the user's `~/.claude/settings.json`. The rest
//! of the suite is `agent_integration_tests.rs`.

use upleft_app::app::setup_window_controller::Step;

/// Every other step is a registration the app makes on its own behalf. The
/// agent hook edits another tool's configuration file, so it is the one step
/// that has to be asked for rather than opted out of.
#[test]
fn agent_step_is_not_preselected() {
    assert!(!Step::AgentIntegration.is_preselected());
    for step in Step::ALL_CASES.into_iter().filter(|step| *step != Step::AgentIntegration) {
        assert!(step.is_preselected());
    }
}

#[test]
fn every_step_explains_itself() {
    for step in Step::ALL_CASES {
        assert!(!step.title().is_empty());
        assert!(!step.detail().is_empty());
        assert!(!step.icon().is_empty());
    }
}

/// The detail line is where the user learns the hook touches a file outside
/// this app, which is the fact that makes the step worth declining.
#[test]
fn agent_step_names_the_file_it_edits() {
    assert!(upleft_swift_text::contains(Step::AgentIntegration.detail(), "settings.json"));
}
