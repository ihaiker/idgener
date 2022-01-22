use std::ffi::OsString;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;

use anyhow::bail;
use log::info;
use merge::Merge;
use merge_yaml_hash::MergeYamlHash;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use structopt_yaml::StructOptYaml;

pub fn overwrite<T>(left: &mut Option<T>, right: Option<T>) {
    if left.is_none() || right.is_some() {
        *left = right;
    }
}

#[derive(Merge, Debug, Clone, Copy, Deserialize, Serialize, StructOpt, StructOptYaml)]
#[structopt(name = "keep-alive")]
pub struct KeepAlive {
    #[structopt(long = "keep-alive-failure-threshold", default_value = "3")]
    #[merge(strategy = merge::num::overwrite_zero)]
    pub failure_threshold: u64,

    #[structopt(long = "keep-alive-period-seconds", default_value = "3")]
    #[merge(strategy = merge::num::overwrite_zero)]
    pub period_seconds: u64,
}

/// 分布式ID生成器。
#[derive(Merge, Debug, Clone, Deserialize, Serialize, StructOpt, StructOptYaml)]
#[structopt(name = "idgend")]
pub struct Options {
    /// config file, default: etc/idgend.yaml
    #[structopt(short = "f", long, env = "IDGEND_CONFIG")]
    #[merge(strategy = overwrite)]
    config: Option<String>,

    /// show debug level info
    #[structopt(short, long, env = "IDGEND_DEBUG")]
    #[merge(strategy = merge::bool::overwrite_false)]
    pub debug: bool,

    /// data file dir.
    #[merge(strategy = overwrite)]
    #[structopt(env = "IDGEND_DATA_DIR", short = "D", long)]
    pub data_dir: Option<String>,

    /// http address
    #[merge(strategy = overwrite)]
    #[structopt(env = "IDGEND_HTTP_ADDRESS", short = "H", long, parse(try_from_str))]
    pub http_address: Option<SocketAddr>,

    /// 使用指定的ID创建集群，ID集群判断的优先级高于组播发现机制
    #[merge(strategy = overwrite)]
    #[structopt(env = "IDGEND_ID", short = "i", long, parse(try_from_str))]
    pub id: Option<u16>,

    /// multicast finder address
    #[merge(strategy = overwrite)]
    #[structopt(
        env = "IDGEND_MULTICAST_ADDRESS",
        short = "M",
        long,
        parse(try_from_str)
    )]
    pub multicast_address: Option<SocketAddr>,

    #[structopt(flatten)]
    pub keep_alive: KeepAlive,
}

const PORT: u16 = 7656;

impl Options {
    pub fn default() -> Self {
        Options {
            config: None,
            debug: false,
            data_dir: Some(String::from("data")),
            http_address: Some(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::UNSPECIFIED,
                PORT,
            ))),
            id: None,
            /*
            multicast_address: Some(SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(234, 4, 10, 24),
                PORT + 1,
            ))),
            */
            multicast_address: None,
            keep_alive: KeepAlive {
                period_seconds: 3,
                failure_threshold: 3,
            },
        }
    }

    pub fn parse() -> anyhow::Result<Options> {
        let args = std::env::args().collect::<Vec<_>>();
        Options::parse_custom_args(args)
    }

    pub fn parse_custom_args<I>(args: I) -> anyhow::Result<Options>
    where
        Self: Sized,
        I: IntoIterator + Clone,
        I::Item: Into<OsString> + Clone,
    {
        let config_name = format!("etc/{}.yaml", env!("CARGO_PKG_NAME"));
        let mut default_config = Options::default();
        let from_args_config = Options::from_iter(args.clone());

        let config: Option<String> = match from_args_config.config {
            Some(ref cfg) => Some(cfg.to_string()), //use defined config file
            //use system default config file
            None if Path::new(&config_name).exists() => Some(config_name),
            _ => None,
        };

        if let Some(ref config) = config {
            info!("use config file: {}", config);

            let mut hash = MergeYamlHash::new();
            hash.merge(serde_yaml::to_string(&default_config)?.as_str());
            hash.merge(std::fs::read_to_string(config)?.as_str());

            let file_config =
                match Options::from_iter_with_yaml(hash.to_string().as_str(), args.clone()) {
                    Ok(t) => t,
                    Err(err) => bail!("read config file error: {}", err),
                };
            default_config.merge(file_config);
        }

        default_config.merge(from_args_config);

        Ok(default_config)
    }
}
