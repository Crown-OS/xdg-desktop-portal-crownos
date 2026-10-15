//! `org.freedesktop.impl.portal.Access`: a yes/no question on an app's behalf.
//! crownplugind asks it before a plugin first uses a sensitive or dangerous
//! capability; the `crownos-level` option picks the warning style.

use std::collections::HashMap;

use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{ObjectServer, interface};

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
        };
        tracing::info!(app_id, title, "access dialog");
        let response = tokio::select! {
            answer = chooser::ask(&question) => match answer {
                Ok(true) => Response::Success,
                Ok(false) => Response::Cancelled,
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
        (response.into(), Results::new())
    }
}
