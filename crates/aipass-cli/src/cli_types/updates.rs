use clap::{Args, Subcommand, ValueEnum};

#[derive(Args)]
pub struct SelfUpdateArgs {
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub target: UpdateTarget,
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub channel: UpdateChannelArg,
    /// Maximum seconds for a network operation or desktop restart.
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=3600), global = true)]
    pub timeout_seconds: u64,
    /// Select a development desktop instance (normally release).
    #[arg(
        long,
        value_enum,
        default_value = "release",
        global = true,
        hide = true
    )]
    pub desktop_instance: DesktopInstance,
    #[command(subcommand)]
    pub command: SelfUpdateCommand,
}

#[derive(Subcommand)]
pub enum SelfUpdateCommand {
    /// Check the selected channel. No installation or password prompt.
    Check,
    /// Install the newest compatible version and verify the resulting runtime.
    Install {
        /// Authorize installation and Agent/desktop restart without prompting.
        #[arg(long, short = 'y', required = true)]
        yes: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum UpdateTarget {
    Auto,
    Desktop,
    Cli,
}
#[derive(Clone, Copy, ValueEnum)]
pub enum UpdateChannelArg {
    Auto,
    Official,
    Beta,
    Nightly,
}
#[derive(Clone, Copy, ValueEnum)]
pub enum DesktopInstance {
    Release,
    Development,
    PackagedDevelopment,
}

#[cfg(test)]
mod tests {
    use crate::*;
    #[test]
    fn self_update_parses_global_json_and_requires_explicit_install_authorization() {
        let cli = Cli::try_parse_from([
            "aipass",
            "self-update",
            "check",
            "--json",
            "--channel",
            "nightly",
            "--target",
            "cli",
        ])
        .unwrap();
        assert!(cli.json);
        assert!(matches!(
            cli.command,
            Command::SelfUpdate(SelfUpdateArgs {
                command: SelfUpdateCommand::Check,
                ..
            })
        ));
        assert!(Cli::try_parse_from(["aipass", "self-update", "install"]).is_err());
        assert!(Cli::try_parse_from(["aipass", "self-update", "install", "--yes"]).is_ok());
        for timeout in ["0", "3601"] {
            assert!(Cli::try_parse_from([
                "aipass",
                "self-update",
                "check",
                "--timeout-seconds",
                timeout
            ])
            .is_err());
        }
    }
}
