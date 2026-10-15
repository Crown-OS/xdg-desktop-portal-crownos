//! `org.freedesktop.impl.portal.Request`: lets the frontend dismiss a dialog
//! the app no longer wants answered.

use std::sync::Arc;

use tokio::sync::Notify;
use zbus::interface;

pub struct Request {
    closed: Arc<Notify>,
}

impl Request {
    pub fn new() -> (Self, Arc<Notify>) {
        let closed = Arc::new(Notify::new());
        (
            Self {
                closed: Arc::clone(&closed),
            },
            closed,
        )
    }
}

#[interface(name = "org.freedesktop.impl.portal.Request")]
impl Request {
    fn close(&self) {
        self.closed.notify_one();
    }
}
