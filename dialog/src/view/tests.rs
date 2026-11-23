use std::sync::mpsc::{Receiver, channel};

use crowntest::{Role, TestApp, role};

use super::root;
use crate::alert::Alert;
use crate::answer::{Answer, Reply};

struct Dialog {
    app: TestApp,
    replies: Receiver<Reply>,
}

impl Dialog {
    fn open(args: &[&str]) -> Self {
        let alert = Alert::parse(args.iter().map(|arg| (*arg).to_owned()))
            .unwrap_or_else(|error| panic!("the test alert does not parse: {error:#}"));
        let (sender, replies) = channel();
        let app = TestApp::with_size(
            root(alert, move |reply| {
                let _ = sender.send(reply);
            }),
            1280.0,
            800.0,
        );
        Self { app, replies }
    }

    fn share_disabled(&mut self) -> bool {
        self.app
            .find(role(Role::Button).name("Share"))
            .states
            .disabled
    }

    fn answers(&self) -> Vec<Answer> {
        self.replies()
            .into_iter()
            .map(|reply| reply.answer)
            .collect()
    }

    fn replies(&self) -> Vec<Reply> {
        self.replies.try_iter().collect()
    }
}

const QUESTION: &[&str] = &[
    "--icon",
    "access",
    "--title",
    "Allow camera?",
    "--message",
    "Files wants the camera.",
    "--choice",
    "grant=Allow",
    "--cancel",
    "Don't Allow",
];

const PICKER: &[&str] = &[
    "--icon",
    "screen",
    "--title",
    "Share your screen",
    "--message",
    "Choose what to share.",
    "--cancel",
    "Cancel",
    "--confirm",
    "Share",
    "--section",
    "Screens",
    "--choice",
    "output:DP-1=Entire Screen",
    "--section",
    "Windows",
    "--choice",
    "window:a=Inbox",
    "--detail",
    "Mail",
];

#[test]
fn a_question_reports_the_chosen_button() {
    let mut dialog = Dialog::open(QUESTION);
    assert!(dialog.app.exists(role(Role::Text).text("Allow camera?")));
    dialog.app.click(role(Role::Button).name("Allow"));
    assert_eq!(dialog.answers(), [Answer::Choice("grant".to_owned())]);
}

#[test]
fn the_cancel_button_declines() {
    let mut dialog = Dialog::open(QUESTION);
    dialog.app.click(role(Role::Button).name("Don't Allow"));
    assert_eq!(dialog.answers(), [Answer::Cancel]);
}

#[test]
fn escape_declines() {
    let mut dialog = Dialog::open(QUESTION);
    dialog.app.pointer_move(10.0, 10.0).press_key("Escape");
    assert_eq!(dialog.answers(), [Answer::Cancel]);
}

#[test]
fn a_picker_confirms_only_a_picked_row_of_the_open_tab() {
    let mut dialog = Dialog::open(PICKER);
    assert!(dialog.share_disabled());

    dialog
        .app
        .click(role(Role::RadioButton).name("Entire Screen"));
    assert!(!dialog.share_disabled());
    dialog.app.click(role(Role::RadioButton).name("Windows"));
    assert!(dialog.share_disabled(), "a tab switch clears the pick");

    dialog.app.click(role(Role::RadioButton).name("Inbox"));
    dialog.app.click(role(Role::Button).name("Share"));
    assert_eq!(dialog.answers(), [Answer::Choice("window:a".to_owned())]);
}

const QUESTION_WITH_OPTIONS: &[&str] = &[
    "--icon",
    "access",
    "--title",
    "Let Notes see your screen?",
    "--message",
    "Notes asks for screen context.",
    "--checkbox",
    "Remember this choice",
    "--list",
    "Notes may see",
    "--item",
    "Window list",
    "--item",
    "Window list and titles",
    "--initial",
    "0",
    "--choice",
    "grant=Allow",
    "--cancel",
    "Don't Allow",
];

#[test]
fn a_question_without_options_reports_no_values() {
    let mut dialog = Dialog::open(QUESTION);
    dialog.app.click(role(Role::Button).name("Allow"));
    assert_eq!(
        dialog.replies(),
        [Reply {
            answer: Answer::Choice("grant".to_owned()),
            option_values: Vec::new(),
        }]
    );
}

#[test]
fn the_options_report_their_initial_values() {
    let mut dialog = Dialog::open(QUESTION_WITH_OPTIONS);
    dialog.app.click(role(Role::Button).name("Allow"));
    assert_eq!(
        dialog.replies(),
        [Reply {
            answer: Answer::Choice("grant".to_owned()),
            option_values: vec!["false".to_owned(), "0".to_owned()],
        }]
    );
}

#[test]
fn changed_options_are_reported_with_a_refusal_too() {
    let mut dialog = Dialog::open(QUESTION_WITH_OPTIONS);
    dialog
        .app
        .click(role(Role::Switch).name("Remember this choice"));
    dialog
        .app
        .click(role(Role::RadioButton).name("Window list and titles"));
    dialog.app.click(role(Role::Button).name("Don't Allow"));
    assert_eq!(
        dialog.replies(),
        [Reply {
            answer: Answer::Cancel,
            option_values: vec!["true".to_owned(), "1".to_owned()],
        }]
    );
}
