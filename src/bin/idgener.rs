use std::sync::Arc;

use anyhow::Context;

use idgener::config::logger;
use idgener::config::Options;
use idgener::embedded;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Options::parse().context("parse config")?;
    logger::init(opt.debug)?;
    log::info!("use config: {:#?}", opt);

    let (tx, _) = tokio::sync::broadcast::channel::<u64>(1);
    let tx = Arc::new(tx);
    let ctrl_tx = tx.clone();
    ctrlc::set_handler(move || {
        log::info!("pause ctrl+c, send close signal");
        ctrl_tx.send(1).unwrap();
    })
    .context("listener ctrl+c error")?;
    embedded(&opt, tx).await
}
