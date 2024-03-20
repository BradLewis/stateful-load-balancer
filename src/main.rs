use std::sync::Arc;
use async_trait::async_trait;

use pingora::server::Server;
use pingora::prelude::*;

pub struct LB(Arc<LoadBalancer<RoundRobin>>);

#[async_trait]
impl ProxyHttp for LB {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {
        ()
    }

    async fn upstream_peer(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<Box<HttpPeer>> {
        let upstream = self.0.select(b"", 256).unwrap();

        println!("upstream: {:?}", upstream);
        let peer = Box::new(HttpPeer::new(upstream, false, "".to_owned()));
        Ok(peer)
    }

    async fn upstream_request_filter(&self, _session: &mut Session, upstream_request: &mut RequestHeader, _ctx: &mut Self::CTX) -> Result<()> {
        upstream_request.insert_header("Host", "one.one.one.one").unwrap();
        Ok(())
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
