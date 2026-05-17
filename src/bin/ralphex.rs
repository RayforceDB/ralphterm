#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ralphterm::cli::run().await
}
