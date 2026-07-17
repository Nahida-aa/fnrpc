use std::sync::Arc;

use fnrpc::error::RpcErr;
use fnrpc::handler::{RawRpcFn, RpcFn};
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::{handle, RpcWebConfig, run};

struct Noop;
impl RpcFn<()> for Noop {
    type Input = ();
    type Output = ();
    const NAME: &'static str = "noop";
    fn exec(_ctx: &(), _input: ()) -> Result<(), RpcErr> {
        Ok(())
    }
}

struct Echo;
impl RpcFn<()> for Echo {
    type Input = String;
    type Output = String;
    const NAME: &'static str = "echo";
    fn exec(_ctx: &(), input: String) -> Result<String, RpcErr> {
        Ok(input)
    }
}

struct RawNoop;
impl RawRpcFn<()> for RawNoop {
    const NAME: &'static str = "raw_noop";
    fn exec(_ctx: &(), _input: &[u8]) -> Result<&'static [u8], RpcErr> {
        Ok(b"ok")
    }
}

fn parse_args() -> u16 {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--port" {
            if let Some(p) = args.get(i + 1) {
                return p.parse().unwrap_or(19111);
            }
        }
    }
    19111
}

#[tokio::main]
async fn main() {
    let port = parse_args();
    let config = RpcWebConfig {
        router: RpcRouterBuilder::<()>::new()
            .query(Noop)
            .query(Echo)
            .raw(RawNoop)
            .build(),
        ctx_from_headers: Arc::new(|_| ()),
    };
    let config = Arc::new(config);

    println!("Starting fnrpc-web server on :{port}");
    run(config, &format!("0.0.0.0:{port}")).await.expect("failed to bind");
}
