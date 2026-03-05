use anyhow::Context;
use clap::{Parser, Subcommand};

use tandem_browser::{
    current_sidecar_status, run_doctor, run_stdio_server, BrowserDoctorOptions,
    BrowserServerOptions,
};

#[derive(Parser, Debug)]
#[command(name = "tandem-browser")]
#[command(version)]
#[command(about = "Chromium browser sidecar and diagnostics for Tandem")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(about = "Run browser readiness diagnostics")]
    Doctor {
        #[arg(long, env = "TANDEM_BROWSER_ENABLED", default_value_t = true)]
        enabled: bool,
        #[arg(long, env = "TANDEM_BROWSER_EXECUTABLE")]
        executable_path: Option<String>,
        #[arg(long, env = "TANDEM_BROWSER_USER_DATA_ROOT")]
        user_data_root: Option<String>,
        #[arg(long, env = "TANDEM_BROWSER_ALLOW_NO_SANDBOX", default_value_t = false)]
        allow_no_sandbox: bool,
        #[arg(long, env = "TANDEM_BROWSER_HEADLESS", default_value_t = true)]
        headless: bool,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    #[command(about = "Serve browser automation requests over stdio")]
    Serve {
        #[arg(long, default_value = "stdio")]
        transport: String,
        #[arg(long, env = "TANDEM_BROWSER_EXECUTABLE")]
        executable_path: Option<String>,
        #[arg(long, env = "TANDEM_BROWSER_USER_DATA_ROOT")]
        user_data_root: Option<String>,
        #[arg(long, env = "TANDEM_BROWSER_ALLOW_NO_SANDBOX", default_value_t = false)]
        allow_no_sandbox: bool,
        #[arg(long, env = "TANDEM_BROWSER_HEADLESS", default_value_t = true)]
        headless: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor {
            enabled,
            executable_path,
            user_data_root,
            allow_no_sandbox,
            headless,
            json,
        } => {
            let mut status = run_doctor(BrowserDoctorOptions {
                enabled,
                headless_default: headless,
                allow_no_sandbox,
                executable_path,
                user_data_root,
            });
            status.sidecar = current_sidecar_status();
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                println!("Browser readiness");
                println!("  Enabled: {}", status.enabled);
                println!("  Runnable: {}", status.runnable);
                println!(
                    "  Sidecar: {}",
                    status
                        .sidecar
                        .path
                        .unwrap_or_else(|| "<unknown>".to_string())
                );
                println!(
                    "  Browser: {}",
                    status
                        .browser
                        .path
                        .unwrap_or_else(|| "<not found>".to_string())
                );
                if let Some(version) = status.browser.version {
                    println!("  Browser version: {}", version);
                }
                if !status.blocking_issues.is_empty() {
                    println!("Blocking issues:");
                    for issue in status.blocking_issues {
                        println!("  - {}: {}", issue.code, issue.message);
                    }
                }
                if !status.recommendations.is_empty() {
                    println!("Recommendations:");
                    for row in status.recommendations {
                        println!("  - {}", row);
                    }
                }
                if !status.install_hints.is_empty() {
                    println!("Install hints:");
                    for row in status.install_hints {
                        println!("  - {}", row);
                    }
                }
            }
        }
        Command::Serve {
            transport,
            executable_path,
            user_data_root,
            allow_no_sandbox,
            headless,
        } => {
            if transport.trim() != "stdio" {
                anyhow::bail!("unsupported transport `{}`", transport);
            }
            let options = BrowserServerOptions {
                executable_path,
                user_data_root,
                allow_no_sandbox,
                headless_default: headless,
            };
            run_stdio_server(options).context("browser stdio server failed")?;
        }
    }
    Ok(())
}
