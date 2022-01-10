use std::net::SocketAddr;
use std::ops::Add;

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use actix_server::Server;
use actix_web::middleware::{Compress, DefaultHeaders, Logger};
use actix_web::rt::System as ActixSystem;
use actix_web::{web, App, HttpServer};
use anyhow::{anyhow, Context};

use futures::executor::block_on;
use futures::future::try_join_all;
use http_client::h1::H1Client;
use http_client::http_types::Mime;
use http_client::{Config, HttpClient, Request};
use lazy_static::lazy_static;
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::cluster::{multicast, Node};
use crate::config;
use crate::generator::Snowflake;
use crate::server::health::health;
use crate::server::nodes::JoinInfo;
use crate::server::routers::route;
use crate::server::AppState;

fn bind(address: &SocketAddr, state: web::Data<AppState>) -> anyhow::Result<Server> {
    let server = HttpServer::new(move || {
        App::new()
            .wrap(DefaultHeaders::new().header("x-idgend-version", "0.2"))
            .wrap(Compress::default())
            .wrap(Logger::new("%a %r %s %b %T"))
            .app_data(state.clone())
            .service(web::resource("/health").to(health))
            .service(route())
    })
    .workers(num_cpus::get() * 4)
    .bind(address)
    .context("bind address error")?
    .run();
    Ok(server)
}

lazy_static! {
    static ref CLIENT: H1Client = {
        let mut client = H1Client::new();
        client
            .set_config(Config::default().set_timeout(Some(Duration::from_millis(700))))
            .unwrap();
        client
    };
}

fn send_register(bind_address: SocketAddr, state: web::Data<AppState>) -> anyhow::Result<()> {
    let mut nodes = state.nodes.write().expect("cloud get node read lock");
    if nodes.self_is_leader() {
        return Ok(());
    }
    log::debug!("send keep-alive");

    let leader_address = nodes.get_leader().unwrap().address;
    let mut req = Request::get(format!("http://{}/api/nodes", leader_address).as_str());
    req.set_content_type(Mime::from_str("application/json").unwrap());
    if let Some(current) = nodes.get_current() {
        req.set_body(JoinInfo {
            address: bind_address,
            current_id: Some(current.id),
            nodes: None,
        });
    } else {
        req.set_body(JoinInfo {
            address: bind_address,
            current_id: None,
            nodes: None,
        });
    }

    let mut resp = block_on(CLIENT.send(req)).map_err(|err| anyhow!(err))?;
    let info = block_on(resp.body_json::<JoinInfo>()).map_err(|err| anyhow!(err))?;
    log::debug!("self id: {}", info.current_id.unwrap());
    if nodes.self_is_none() {
        let _ = init_self(&bind_address, state.clone(), info.current_id);
    }
    for node in info.nodes.unwrap().iter() {
        nodes.join(node.clone());
    }
    Ok(())
}

fn change_new_leader(state: web::Data<AppState>) -> anyhow::Result<()> {
    let mut nodes = state.nodes.write().expect("cloud get node read lock");
    let leader = nodes.get_leader().unwrap();
    if let Some(&node) = &nodes.next(leader.id) {
        log::info!("set new leader: {:?}", node);
        nodes.set_leader(Some(node.id));
    }
    Ok(())
}

async fn register(
    config: config::KeepAlive,
    bind_address: SocketAddr,
    state: web::Data<AppState>,
    mut stopper: broadcast::Receiver<u64>,
) -> anyhow::Result<()> {
    let mut timer_interval = tokio::time::interval_at(
        Instant::now().add(Duration::from_secs(config.period_seconds)),
        Duration::from_secs(config.period_seconds),
    );
    log::info!("start send keep-alive heartbeat");

    let mut fail_num = config.failure_threshold;

    loop {
        tokio::select! {
            _ = stopper.recv() => {
                log::debug!("close keep-alive");
                break;
            },
            _ = timer_interval.tick() => {
                match send_register(bind_address.clone(), state.clone()) {
                    Ok(_) => {
                        fail_num = config.failure_threshold;
                    }
                    Err(_) if fail_num == 0 => {
                        if let Ok(_) = change_new_leader(state.clone()) {
                            //fail_num = config.failure_threshold;
                        }
                    }
                    Err(err) => {
                        fail_num -= 1;
                        log::warn!("send keep-alive: {}", err);
                    }
                }
            }
        }
    }
    Ok(())
}

/// 初始化当前node，如果[self_id]为[None]本机会自动设置为leader
fn init_self(
    bind_address: &SocketAddr,
    state: web::Data<AppState>,
    self_id: Option<u16>,
) -> anyhow::Result<()> {
    let ip = multicast::local_ipaddress().context("not found local ip")?;
    let bind_address = SocketAddr::new(ip, (&bind_address).port());
    log::info!("self node {} make cluster", &bind_address);
    let current_id = self_id.or(Some(0)).unwrap();

    let mut nodes = state.nodes.write().expect("could get nodes write lock");
    nodes
        .join(Node::new(current_id, bind_address.clone()))
        .set_current(current_id);

    if self_id.is_none() {
        nodes.set_leader(Some(0));
    }

    let mut snowflake = state.snowflake.write().expect("could get snowflake lock");
    if snowflake.is_none() {
        let _ = snowflake.insert(Snowflake::new(current_id));
    }
    Ok(())
}

pub async fn embedded(
    config: &config::Options,
    stopper: Arc<broadcast::Sender<u64>>,
) -> anyhow::Result<()> {
    let bind_address = config.http_address.context("can't found http address")?;
    let mut sys = ActixSystem::new("idgener");

    let state = web::Data::new(AppState::default());
    let server = bind(&bind_address, state.clone())?;

    let mut futures = vec![];
    if let Some(id) = &config.id {
        log::info!("make cluster, self id: {}", id);
        let _ = init_self(&bind_address, state.clone(), Some(*id))?;
    } else if let Some(multicast_address) = &config.multicast_address {
        log::info!("find multicast address: {}", &multicast_address);
        let timeout = Some(Duration::from_secs(
            config.keep_alive.period_seconds.clone(),
        ));
        match multicast::finder(&multicast_address, timeout)? {
            Some((leader_address, leader_id)) => {
                log::info!("find cluster leader: {}", leader_address);
                state
                    .nodes
                    .write()
                    .unwrap()
                    .join(Node::new(leader_id, leader_address))
                    .set_leader(Some(leader_id));
            }
            None => {
                log::info!("not find cluster leader");
                init_self(&bind_address, state.clone(), None)?;
            }
        }

        // start cluster listener
        futures.push(tokio::spawn(multicast::listener(
            multicast_address.clone(),
            state.clone(),
            stopper.subscribe(),
        )));

        // start registry
        futures.push(tokio::spawn(register(
            config.keep_alive.clone(),
            bind_address.clone(),
            state.clone(),
            stopper.subscribe(),
        )));
    } else {
        init_self(&bind_address, state.clone(), None)?;
    }

    if futures.len() == 0 {
        let mut rc = stopper.subscribe();
        futures.push(tokio::spawn(async move {
            let _ = rc.recv().await;
            Ok(())
        }));
    }

    let mut stopper = stopper.subscribe();
    let srv = server.clone();

    tokio::select! {
        out = try_join_all(futures) => {
            if let Err(out) = out {
                log::info!("error: {:#?}", out);
                sys.block_on(srv.stop(true))
            }
        }
        _ = server => {
            log::info!("server stop")
        }
        _ = stopper.recv() => {
            log::info!("user close server");
            sys.block_on(srv.stop(true))
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use std::net::SocketAddr;
    use std::thread;
    use std::time::Duration;

    use actix_web::web;
    use anyhow::Context;
    use futures::executor::block_on;
    use http_client::http_types::StatusCode;
    use http_client::{HttpClient, Request};

    use crate::config::logger;
    use crate::server::server::{bind, CLIENT};
    use crate::server::AppState;

    #[actix_rt::test]
    async fn test_bind() -> anyhow::Result<()> {
        logger::init(true);

        let (tx, rx) = std::sync::mpsc::channel();

        let state = web::Data::new(AppState::default());
        let address = "127.0.0.1:8080"
            .parse::<SocketAddr>()
            .expect("invalid address");
        let server = bind(&address, state).expect("Can't run server");

        thread::spawn(move || {
            thread::sleep(Duration::from_secs(1));
            log::info!("send close");
            tx.send(()).expect("send error");
        });

        let srv = server.clone();
        thread::spawn(move || {
            // wait for shutdown signal
            rx.recv().unwrap();
            log::info!("close server");
            // stop server gracefully
            block_on(srv.stop(true));
            log::info!("closed server");
        });

        server.await.context("runner server")
    }

    #[test]
    fn http_client() {
        let req = Request::get("http://ip-api.com/json");
        let resp = block_on(CLIENT.send(req));
        assert!(resp.is_ok());
        let resp = resp.unwrap();
        assert_eq!(StatusCode::Ok, resp.status());
    }
}
