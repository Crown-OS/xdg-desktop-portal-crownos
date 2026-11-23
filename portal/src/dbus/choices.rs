//! The `choices` option of the Access dialog, in the FileChooser format: an
//! `a(ssa(ss)s)` of questions in, an `a(ss)` of answers out. A question with
//! no items is a checkbox that answers `true` or `false`.
//!
//! Ids and labels come from the calling app. The dialog only gets labels and
//! gives back indices, and the ids are put back here.

use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

pub const CHOICES: &str = "choices";
const TRUE: &str = "true";
const FALSE: &str = "false";

type ChoiceSpecification = (String, String, Vec<(String, String)>, String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessChoice {
    id: String,
    label: String,
    items: Vec<(String, String)>,
    initial: String,
}

impl AccessChoice {
    fn is_checkbox(&self) -> bool {
        self.items.is_empty()
    }

    fn initial_item(&self) -> usize {
        self.items
            .iter()
            .position(|(id, _)| *id == self.initial)
            .unwrap_or_default()
    }
}

/// The choices the caller asked, or none when the option is missing or has
/// another type.
pub fn read_choices(options: &HashMap<String, OwnedValue>) -> Vec<AccessChoice> {
    let Some(value) = options.get(CHOICES) else {
        return Vec::new();
    };
    match <Vec<ChoiceSpecification>>::try_from(value.clone()) {
        Ok(specifications) => specifications
            .into_iter()
            .map(|(id, label, items, initial)| AccessChoice {
                id,
                label,
                items,
                initial,
            })
            .collect(),
        Err(error) => {
            tracing::warn!("ignoring access dialog choices: {error}");
            Vec::new()
        }
    }
}

/// The dialog flags that ask `choices`, in order.
pub fn dialog_arguments(choices: &[AccessChoice]) -> Vec<String> {
    let mut arguments = Vec::new();
    for choice in choices {
        if choice.is_checkbox() {
            let initially_on = if choice.initial == TRUE { TRUE } else { FALSE };
            arguments.extend([
                "--checkbox".to_owned(),
                choice.label.clone(),
                "--initial".to_owned(),
                initially_on.to_owned(),
            ]);
        } else {
            arguments.extend(["--list".to_owned(), choice.label.clone()]);
            for (_, label) in &choice.items {
                arguments.extend(["--item".to_owned(), label.clone()]);
            }
            arguments.extend(["--initial".to_owned(), choice.initial_item().to_string()]);
        }
    }
    arguments
}

/// Puts the ids back on the values the dialog printed. `None` unless every
/// choice has a valid value.
pub fn answers(choices: &[AccessChoice], values: &[String]) -> Option<Vec<(String, String)>> {
    if choices.len() != values.len() {
        return None;
    }
    choices
        .iter()
        .zip(values)
        .map(|(choice, value)| {
            let answer = if choice.is_checkbox() {
                [TRUE, FALSE]
                    .into_iter()
                    .find(|known| known == value)?
                    .to_owned()
            } else {
                let index: usize = value.parse().ok()?;
                choice.items.get(index)?.0.clone()
            };
            Some((choice.id.clone(), answer))
        })
        .collect()
}

/// `answers` as the `a(ss)` the `choices` result holds.
pub fn result_value(answers: Vec<(String, String)>) -> Option<OwnedValue> {
    OwnedValue::try_from(Value::new(answers)).ok()
}

#[cfg(test)]
mod tests {
    use zbus::zvariant::{Signature, Type, serialized::Context, to_bytes};

    use super::*;

    fn remember_and_scope() -> zbus::zvariant::Result<Vec<AccessChoice>> {
        let specifications: Vec<ChoiceSpecification> = vec![
            (
                "remember".to_owned(),
                "Remember this choice".to_owned(),
                Vec::new(),
                "true".to_owned(),
            ),
            (
                "scope".to_owned(),
                "Notes may see".to_owned(),
                vec![
                    ("metadata".to_owned(), "Window list".to_owned()),
                    ("text".to_owned(), "Window titles".to_owned()),
                ],
                "text".to_owned(),
            ),
        ];
        let options = HashMap::from([(
            CHOICES.to_owned(),
            OwnedValue::try_from(Value::new(specifications))?,
        )]);
        Ok(read_choices(&options))
    }

    #[test]
    fn choices_are_read_from_the_options() -> zbus::zvariant::Result<()> {
        let choices = remember_and_scope()?;
        assert_eq!(choices.len(), 2);
        assert!(choices[0].is_checkbox());
        assert_eq!(choices[1].initial_item(), 1);
        assert!(read_choices(&HashMap::new()).is_empty());

        let wrong_type = HashMap::from([(CHOICES.to_owned(), OwnedValue::from(7u32))]);
        assert!(read_choices(&wrong_type).is_empty());
        Ok(())
    }

    #[test]
    fn the_dialog_gets_labels_and_indices_but_no_ids() -> zbus::zvariant::Result<()> {
        let arguments = dialog_arguments(&remember_and_scope()?);
        assert_eq!(
            arguments,
            [
                "--checkbox",
                "Remember this choice",
                "--initial",
                "true",
                "--list",
                "Notes may see",
                "--item",
                "Window list",
                "--item",
                "Window titles",
                "--initial",
                "1",
            ]
        );
        assert!(dialog_arguments(&[]).is_empty());
        Ok(())
    }

    #[test]
    fn dialog_values_map_back_to_ids() -> zbus::zvariant::Result<()> {
        let choices = remember_and_scope()?;
        let values = ["false".to_owned(), "0".to_owned()];
        assert_eq!(
            answers(&choices, &values),
            Some(vec![
                ("remember".to_owned(), "false".to_owned()),
                ("scope".to_owned(), "metadata".to_owned()),
            ])
        );
        assert_eq!(answers(&choices, &values[..1]), None);
        assert_eq!(answers(&choices, &["yes".to_owned(), "0".to_owned()]), None);
        assert_eq!(
            answers(&choices, &["true".to_owned(), "9".to_owned()]),
            None
        );
        Ok(())
    }

    #[test]
    fn the_result_is_an_array_of_string_pairs() -> zbus::zvariant::Result<()> {
        let pairs = vec![("remember".to_owned(), "true".to_owned())];
        for answers in [pairs, Vec::new()] {
            let value =
                result_value(answers.clone()).ok_or(zbus::zvariant::Error::IncorrectType)?;
            assert_eq!(value.value_signature(), &Signature::try_from("a(ss)")?);
            assert_eq!(<Vec<(String, String)>>::try_from(value)?, answers);
        }
        let results = HashMap::from([(
            CHOICES.to_owned(),
            result_value(Vec::new()).ok_or(zbus::zvariant::Error::IncorrectType)?,
        )]);
        type Reply = (u32, HashMap<String, OwnedValue>);
        assert_eq!(<Reply as Type>::SIGNATURE.to_string(), "(ua{sv})");
        let context = Context::new_dbus(zbus::zvariant::LE, 0);
        let body = to_bytes(context, &(0u32, results))?;
        let (response, read_back): Reply = body.deserialize()?.0;
        assert_eq!(response, 0);
        let choices = read_back
            .get(CHOICES)
            .ok_or(zbus::zvariant::Error::IncorrectType)?;
        assert_eq!(choices.value_signature().to_string(), "a(ss)");
        Ok(())
    }
}
