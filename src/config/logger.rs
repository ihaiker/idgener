use std::path::Path;

use anyhow::Context;
use log4rs::config::RawConfig;

pub fn init(debug: bool) -> anyhow::Result<()> {
    let log_file = Path::new("etc/log4rs.yaml");
    if log_file.exists() {
        log4rs::init_file(log_file, Default::default()).context("log config file error")?;
    } else {
        log_default()?;
    }
    if debug {
        log::set_max_level(log::LevelFilter::Debug);
    }
    Ok(())
}

fn log_default() -> anyhow::Result<()> {
    let cfg = r#"
refresh_rate: 30 seconds
appenders:
  stdout:
    kind: console
    encoder:
      pattern: "{h({l})} {d(%T)(local)} [{t}] {m}{n}"

root:
  level: info
  appenders:
    - stdout
"#;
    let cfg = serde_yaml::from_str::<RawConfig>(cfg).unwrap();
    log4rs::init_raw_config(cfg).context("init log config")?;
    Ok(())
}
