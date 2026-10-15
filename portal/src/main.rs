mod cast;
mod chooser;
mod dbus;
mod util;

use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let (ended, ended_casts) = tokio::sync::mpsc::unbounded_channel();
    let caster = cast::Caster::spawn(ended)?;
    dbus::serve(caster, ended_casts).await
}
