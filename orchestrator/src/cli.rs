use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "orchestrator", about = "Claude Code agent orchestrator plugin")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the daemon if not running; wait until the socket is ready.
    Bootstrap,
    /// Read a hook event from stdin, forward to daemon, write response to stdout.
    Hook,
    /// Run the daemon (socket listener + event loop).
    Daemon,
    /// Launch the terminal UI.
    Tui,
    /// Run the MCP server (stdio JSON-RPC).
    Mcp,
    /// Stop the running daemon.
    Stop,
    /// Show daemon status.
    Status,
    /// Install hooks into Claude Code settings.
    Install {
        /// Install to project settings (.claude/settings.json) instead of user settings.
        #[arg(long)]
        project: bool,
        /// Uninstall: remove orchestrator hooks from settings.
        #[arg(long)]
        uninstall: bool,
    },
}
