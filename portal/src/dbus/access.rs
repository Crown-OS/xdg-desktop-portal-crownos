//! `org.freedesktop.impl.portal.Access`: a yes/no question on an app's behalf.
//! crownplugind asks it before a plugin first uses a sensitive or dangerous
//! capability, and crownpositor before an app gets agent access; the
//! `crownos-level` option picks the warning style. The `choices` option adds
//! questions, such as "Remember this choice", whose answers come back in the
//! `choices` result when the user answers with either button.

use std::collections::HashMap;

use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{ObjectServer, interface};

use super::choices::{self, CHOICES};
use super::request::Request;
use super::response::Response;
use crate::chooser;

const LEVEL: &str = "crownos-level";

type Results = HashMap<String, OwnedValue>;

pub struct Access;

fn option(options: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(|value| String::try_from(value.clone()).ok())
}

#[interface(name = "org.freedesktop.impl.portal.Access")]
impl Access {
    #[allow(clippy::too_many_arguments)]
    async fn access_dialog(
        &self,
        handle: OwnedObjectPath,
        app_id: &str,
        _parent_window: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        options: HashMap<String, OwnedValue>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> (u32, Results) {
        let (request, closed) = Request::new();
        let registered = server.at(&handle, request).await.unwrap_or(false);
        let asked_choices = choices::read_choices(&options);
        let option_arguments = choices::dialog_arguments(&asked_choices);
        let question = chooser::Question {
            dangerous: option(&options, LEVEL).as_deref() == Some("dangerous"),
            title,
            message: &[subtitle, body]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
            grant: &option(&options, "grant_label").unwrap_or_else(|| "Allow".to_owned()),
            deny: &option(&options, "deny_label").unwrap_or_else(|| "Don't Allow".to_owned()),
            option_arguments: &option_arguments,
            option_count: asked_choices.len(),
        };
        tracing::info!(app_id, title, "access dialog");
        let mut results = Results::new();
        let response = tokio::select! {
            answer = chooser::ask(&question) => match answer {
                Ok(answered) => {
                    let answers = answered
                        .option_values
                        .filter(|_| !asked_choices.is_empty())
                        .and_then(|values| choices::answers(&asked_choices, &values))
                        .and_then(choices::result_value);
                    if let Some(answers) = answers {
                        results.insert(CHOICES.to_owned(), answers);
                    }
                    if answered.granted {
                        Response::Success
                    } else {
                        Response::Cancelled
                    }
                }
                Err(error) => {
                    tracing::warn!("access dialog failed: {error:#}");
                    Response::Failed
                }
            },
            () = closed.notified() => Response::Cancelled,
        };
        if registered {
            let _ = server.remove::<Request, _>(&handle).await;
        }
        (response.into(), results)
    }
}
