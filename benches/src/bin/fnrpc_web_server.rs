use std::sync::Arc;

use fnrpc::error::RpcErr;
use fnrpc::handler::RpcFn;
use fnrpc::router::RpcRouterBuilder;
use fnrpc_web::{handle, FnrpcConfig, run};

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

#[tokio::main]
async fn main() {
    let config = FnrpcConfig {
        router: RpcRouterBuilder::<()>::new().query(Noop).query(Echo).build(),
        ctx_from_headers: Arc::new(|_| ()),
    };
    let config = Arc::new(config);

    println!("Starting fnrpc-web server on :19111");
    run(config, "0.0.0.0:19111").await.expect("failed to bind");
}
