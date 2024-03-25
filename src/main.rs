use async_trait::async_trait;
use pingora::upstreams::peer::Peer;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pingora::connectors::http::Connector;
use pingora::lb::health_check::HealthCheck;
use pingora::lb::selection::Consistent;
use pingora::lb::Backend;
use pingora::prelude::*;
use pingora::server::Server;
use regex::Regex;

pub struct LB(Arc<LoadBalancer<Consistent>>);

pub struct AppState {
    pub map: BTreeMap<usize, Backend>,
}

pub struct Context {
    pub worker_id: Option<usize>,
}

pub struct StatefulHealthCheck {
    state: Arc<Mutex<AppState>>,
    pub peer_template: HttpPeer,
    pub reuse_connection: bool,
    pub req: RequestHeader,
    pub connector: Connector,
}

impl StatefulHealthCheck {
    pub fn new(host: &str, tls: bool, state: Arc<Mutex<AppState>>) -> Self {
        let mut req = RequestHeader::build("GET", b"/health", None).unwrap();
        req.append_header("Host", host).unwrap();
        let sni = if tls { host.into() } else { String::new() };
        let mut peer_template = HttpPeer::new("0.0.0.0:1", tls, sni);
        peer_template.options.connection_timeout = Some(Duration::from_secs(1));
        peer_template.options.read_timeout = Some(Duration::from_secs(1));
        StatefulHealthCheck {
            state,
            peer_template,
            reuse_connection: true,
            connector: Connector::new(None),
            req,
        }
    }
}

#[async_trait]
impl HealthCheck for StatefulHealthCheck {
    async fn check(&self, target: &Backend) -> Result<()> {
        let mut peer = self.peer_template.clone();
        peer._address = target.addr.clone();
        let session = self.connector.get_http_session(&peer).await?;

        let mut session = session.0;
        let req = Box::new(self.req.clone());
        session.write_request_header(req).await?;

        if let Some(read_timeout) = peer.options.read_timeout {
            session.set_read_timeout(read_timeout);
        }

        session.read_response_header().await?;

        let resp = session.response_header().expect("just read");

        if resp.status != 200 {
            return Error::e_explain(
                CustomCode("non 200 code", resp.status.as_u16()),
                "during http healthcheck",
            );
        };

        let workers = match session.read_response_body().await? {
            Some(body) => {
                let workers: Vec<usize> = serde_json::from_slice(&body).unwrap();
                workers
            }
            None => Vec::new(),
        };

        for worker_id in workers {
            self.state
                .lock()
                .unwrap()
                .map
                .insert(worker_id, target.clone());
        }

        if self.reuse_connection {
            let idle_timeout = peer.idle_timeout();
            self.connector
                .release_http_session(session, &peer, idle_timeout)
                .await;
        }

        Ok(())
    }

    fn health_threshold(&self, success: bool) -> usize {
        if success {
            1
        } else {
            0
        }
    }
}

fn get_worker_id(uri: &str) -> Option<usize> {
    let re = match Regex::new(r"/worker/(\d+)/?.*") {
        Ok(re) => re,
        Err(e) => {
            eprintln!("Error creating regex, {:?}", e);
            return None;
        }
    };

    let caps = match re.captures(uri) {
        Some(caps) => caps,
        None => {
            eprintln!("No captures found");
            return None;
        }
    };

    match caps.get(1)?.as_str().parse::<usize>() {
        Ok(worker_id) => Some(worker_id),
        Err(e) => {
            eprintln!("Error parsing worker_id, {:?}", e);
            None
        }
    }
}

pub struct StatefulLoadBalancer {
    state: Arc<Mutex<AppState>>,
    upstreams: Arc<LoadBalancer<Consistent>>,
}

impl StatefulLoadBalancer {
    pub fn new(state: Arc<Mutex<AppState>>, upstreams: Arc<LoadBalancer<Consistent>>) -> Self {
        StatefulLoadBalancer { state, upstreams }
    }
}

#[async_trait]
impl ProxyHttp for StatefulLoadBalancer {
    type CTX = Context;

    fn new_ctx(&self) -> Self::CTX {
        Context { worker_id: None }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        println!("received request for worker_id: {:?}", ctx.worker_id);
        let worker_id = ctx.worker_id.unwrap_or(0);
        let upstream = match self.state.lock().unwrap().map.get(&worker_id) {
            Some(backend) => backend.addr.clone(),
            None => self
                .upstreams
                .select(&worker_id.to_be_bytes(), 256)
                .unwrap()
                .addr
                .clone(),
        };

        let peer = Box::new(HttpPeer::new(upstream, false, "".to_owned()));
        Ok(peer)
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        let uri = &session.req_header().uri.path();
        if uri.starts_with("/worker") {
            ctx.worker_id = get_worker_id(uri);
        }
        Ok(false)
    }
}

fn main() {
    let mut my_server = Server::new(None).unwrap();
    my_server.bootstrap();

    let mut upstreams =
        LoadBalancer::try_from_iter(["localhost:3000", "localhost:3001", "localhost:3002"])
            .unwrap();
    let state = Arc::new(Mutex::new(AppState {
        map: BTreeMap::new(),
    }));

    let hc = Box::new(StatefulHealthCheck::new("localhost", false, state.clone()));
    upstreams.set_health_check(hc);
    upstreams.health_check_frequency = Some(std::time::Duration::from_secs(1));

    let bg = background_service("health check", upstreams);

    let upstreams = bg.task();
    let mut lb = http_proxy_service(
        &my_server.configuration,
        StatefulLoadBalancer::new(state, upstreams),
    );

    lb.add_tcp("0.0.0.0:6188");

    my_server.add_service(bg);
    my_server.add_service(lb);

    my_server.run_forever();
}
