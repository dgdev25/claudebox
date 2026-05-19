use claudebox_vec::mcp_server::{build_port_change_message, find_mcp_port};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let preferred_port: u16 = std::env::var("CLAUDEBOX_MCP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7878);

    let actual_port = find_mcp_port(preferred_port).await;

    if actual_port != preferred_port {
        // Notify host via stdout (captured by vsock bridge)
        println!("{}", build_port_change_message(actual_port, preferred_port));
    }

    // MCP stdio server — full tool dispatch deferred to Phase 8 wiring
    // Listens for JSON-RPC requests on stdin, responds on stdout
    eprintln!("claudebox-mcp starting on port {actual_port}");

    // Keep alive until killed
    tokio::signal::ctrl_c().await?;
    Ok(())
}
