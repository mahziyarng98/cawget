//! Runs the test server until killed; handy for trying the CLI or GUI by hand:
//!   cargo run -p cawget-testserver --release   then use  <printed base>/data/<bytes>?bps=<bytes/s>

#[tokio::main]
async fn main() {
    let s = cawget_testserver::start().await;
    println!("{}", s.base);
    std::future::pending::<()>().await;
}
