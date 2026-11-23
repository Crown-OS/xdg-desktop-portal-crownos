use std::sync::mpsc::{Receiver, channel};

use crowntest::{Role, TestApp, role};

use super::root;
use crate::alert::Alert;
use crate::answer::Answer;

struct Dialog {
    app: TestApp,
    answers: Receiver<Answer>,
}

impl Dialog {
    fn open(args: &[&str]) -> Self {
        let alert = Alert::parse(args.iter().map(|arg| (*arg).to_owned()))
            .unwrap_or_else(|error| panic!("the test alert does not parse: {error:#}"));
        let (sender, answers) = channel();
        let app = TestApp::with_size(
            root(alert, move |answer| {
                let _ = sender.send(answer);
            }),
            1280.0,
            800.0,
        );
        Self { app, answers }
    }

    fn share_disabled(&mut self) -> bool {
        self.app
            .find(role(Role::Button).name("Share"))
            .states
            .disabled
    }

    fn answers(&self) -> Vec<Answer> {
        self.answers.try_iter().collect()
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
