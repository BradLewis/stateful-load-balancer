use std::sync::Arc;
use async_trait::async_trait;

use pingora::server::Server;
use pingora::prelude::*;
use regex::Regex;

pub struct LB(Arc<LoadBalancer<RoundRobin>>);

pub struct Context {
   pub worker_id: Option<usize>,
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

#[async_trait]
impl ProxyHttp for LB {
    type CTX = Context;

    fn new_ctx(&self) -> Self::CTX {
        Context {
            worker_id: None,
        }
    }

    async fn upstream_peer(&self, _session: &mut Session, ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        println!("received request for worker_id: {:?}", ctx.worker_id);
        let upstream = self.0.select(b"", 256).unwrap();

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
        LoadBalancer::try_from_iter(["localhost:3000", "localhost:3001", "localhost:3002"]).unwrap();

    let hc = TcpHealthCheck::new();
    upstreams.set_health_check(hc);
    upstreams.health_check_frequency = Some(std::time::Duration::from_secs(1));

    let bg = background_service("health check", upstreams);

    let upstreams = bg.task();
    let mut lb = http_proxy_service(&my_server.configuration, LB(upstreams));
    
    lb.add_tcp("0.0.0.0:6188");

    my_server.add_service(bg);
    my_server.add_service(lb);

    my_server.run_forever();
}
