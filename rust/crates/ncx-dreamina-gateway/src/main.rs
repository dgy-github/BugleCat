use ncx_dreamina_gateway::{admin_router, api_router, AppState, GatewayConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = GatewayConfig::from_env()?;
    let state = AppState::load(config.state_path.clone()).await?;

    let api_listener = tokio::net::TcpListener::bind(config.api_addr).await?;
    let admin_listener = tokio::net::TcpListener::bind(config.admin_addr).await?;

    println!("ncx-dreamina-gateway API listening on http://{}", config.api_addr);
    println!(
        "ncx-dreamina-gateway admin listening on http://{}",
        config.admin_addr
    );
    println!("state file: {}", config.state_path.display());
    println!("provider mode: mock (no real Dreamina requests are sent)");

    let api = axum::serve(api_listener, api_router(state.clone()));
    let admin = axum::serve(admin_listener, admin_router(state));

    tokio::select! {
        result = api => result?,
        result = admin => result?,
        _ = tokio::signal::ctrl_c() => {
            println!("shutdown requested");
        }
    }

    Ok(())
}
