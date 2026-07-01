mod app;
pub mod backend;
pub mod editor;
pub mod renderer;
pub mod tools;
pub mod types;
pub mod utils;

#[tokio::main]

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wayland_conn = wayland_client::Connection::connect_to_env().ok();
    app::make_screenshot(wayland_conn).await?;
    Ok(())
}
