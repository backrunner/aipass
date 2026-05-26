pub mod client;
pub mod desktop;
pub mod ipc;
pub mod paths;
pub mod server;
pub mod session;
pub mod windows_service;

pub use client::{AgentClient, AgentClientConfig, AgentCommandError};
pub use paths::{
    agent_service_name, agent_socket_path, canonical_vault_dir, default_vault_dir,
    namespace_for_vault_dir,
};
pub use server::{run_server, ServerOptions};
pub use windows_service::{
    install_service as install_windows_service, query_service as query_windows_service,
    run_dispatcher as run_windows_service_dispatcher, start_service as start_windows_service,
    stop_service as stop_windows_service, WindowsServiceStatus,
};
