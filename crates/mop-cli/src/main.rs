use clap::{Parser, Subcommand};
use mop_core::config::Config;
use mop_core::error::AppError;
use mop_db::{create_sqlite_pool, run_migrations};
use mop_http::create_app;
use mop_watch::{CompositeCollector, FakeResourceCollector, ResourceCollector};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(
    name = "mop",
    author,
    version,
    about = "master-of-process daemon and management tool"
)]
struct Cli {
    #[arg(short, long, global = true, help = "Path to configuration file")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Start the mop server (default)")]
    Serve {
        #[arg(long, help = "Bind address override (e.g. 127.0.0.1:8787)")]
        bind: Option<String>,

        #[arg(long, help = "Database path override")]
        db_path: Option<PathBuf>,

        #[arg(long, help = "Force using fake resource collector for testing")]
        fake_backend: bool,

        #[arg(long, help = "Plugins directory override")]
        plugins_dir: Option<PathBuf>,

        #[arg(long, help = "Plugins run directory override (sockets)")]
        plugins_run_dir: Option<PathBuf>,
    },

    #[command(about = "Generate polkit rules JavaScript from allowlist (SPEC.md §9.1)")]
    PolkitRules {
        #[arg(short, long, help = "Output file path (default: stdout)")]
        output: Option<PathBuf>,
    },

    #[command(about = "Run system diagnosis")]
    Doctor,

    #[command(about = "Create or list backups (M6)")]
    Backup {
        #[arg(long, help = "Create a new backup")]
        create: bool,
    },

    #[command(about = "Restore database and config from backup file (M6)")]
    Restore {
        #[arg(help = "Path to backup archive")]
        file: PathBuf,
    },

    #[command(about = "Manage mop plugins (M4)")]
    Plugin {
        #[command(subcommand)]
        action: Option<PluginCommands>,
    },
}

#[derive(Subcommand, Debug)]
enum PluginCommands {
    #[command(about = "List installed plugins")]
    List,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let mut config = Config::load_from_file_or_default(cli.config.as_deref())?;

    match cli.command.unwrap_or(Commands::Serve {
        bind: None,
        db_path: None,
        fake_backend: false,
        plugins_dir: None,
        plugins_run_dir: None,
    }) {
        Commands::Serve {
            bind,
            db_path,
            fake_backend,
            plugins_dir,
            plugins_run_dir,
        } => {
            if let Some(b) = bind {
                config.server.bind = b;
            }
            if let Some(db) = db_path {
                config.database.path = db;
            }
            if fake_backend {
                config.resources.fake = true;
            }
            if let Some(p) = plugins_dir {
                config.plugins.dir = p;
            }
            if let Some(r) = plugins_run_dir {
                config.plugins.run_dir = r;
            }

            run_server(config).await?;
        }
        Commands::PolkitRules { output } => {
            let rules = generate_polkit_rules(&config);
            if let Some(out_path) = output {
                std::fs::write(&out_path, &rules)?;
                println!("Polkit rules written to {}", out_path.display());
            } else {
                println!("{rules}");
            }
        }
        Commands::Doctor => {
            println!("mop version: {}", env!("CARGO_PKG_VERSION"));
            println!("Config database path: {}", config.database.path.display());
            println!("Config server bind: {}", config.server.bind);
            println!("Registration mode: {}", config.auth.registration);
            println!(
                "Allowlisted systemd units: {:?}",
                config.resources.systemd.units
            );
            println!(
                "Allowlisted docker containers: {:?}",
                config.resources.docker.containers
            );

            // Polkit rules diagnosis
            let polkit_path = Path::new("/etc/polkit-1/rules.d/50-mop.rules");
            if polkit_path.exists() {
                if let Ok(installed) = std::fs::read_to_string(polkit_path) {
                    let expected = generate_polkit_rules(&config);
                    if installed.trim() == expected.trim() {
                        println!(
                            "Polkit rules: [OK] Synchronized at {}",
                            polkit_path.display()
                        );
                    } else {
                        println!("Polkit rules: [WARNING] Installed rules differ from current allowlist.");
                        println!(
                            "  To update, run: sudo mop polkit-rules --output {}",
                            polkit_path.display()
                        );
                    }
                }
            } else {
                println!(
                    "Polkit rules: [NOTICE] File not found at {}",
                    polkit_path.display()
                );
                println!(
                    "  To generate and install, run: sudo mop polkit-rules --output {}",
                    polkit_path.display()
                );
            }

            println!("Doctor diagnosis complete: System is healthy.");
        }
        Commands::Backup { create: _ } => {
            eprintln!("Error: 'mop backup' is a stub in milestone M2. It will be implemented in milestone M6 (SPEC.md §14).");
            std::process::exit(1);
        }
        Commands::Restore { file: _ } => {
            eprintln!("Error: 'mop restore' is a stub in milestone M2. It will be implemented in milestone M6 (SPEC.md §14).");
            std::process::exit(1);
        }
        Commands::Plugin { action: _ } => {
            eprintln!("Error: 'mop plugin' is a stub in milestone M2. It will be implemented in milestone M4 (SPEC.md §11).");
            std::process::exit(1);
        }
    }

    Ok(())
}

fn generate_polkit_rules(config: &Config) -> String {
    let units_json = serde_json::to_string_pretty(&config.resources.systemd.units)
        .unwrap_or_else(|_| "[]".to_string());
    format!(
        r#"// Polkit rules for mop daemon (SPEC.md §9.1)
// Generated automatically from mop config.toml allowlist

polkit.addRule(function(action, subject) {{
    if (action.id == "org.freedesktop.systemd1.manage-units" &&
        subject.user == "mop") {{
        
        var unit = action.lookup("unit");
        var allowedUnits = {units_json};

        if (allowedUnits.indexOf(unit) >= 0) {{
            return polkit.Result.YES;
        }}
    }}
}});
"#
    )
}

async fn run_server(config: Config) -> Result<(), AppError> {
    info!(
        "Initializing database at {}",
        config.database.path.display()
    );
    let pool = create_sqlite_pool(&config.database.path).await?;

    info!("Running database migrations...");
    run_migrations(&pool).await?;

    let collector: Arc<dyn ResourceCollector> = if config.resources.fake {
        warn!("=====================================================");
        warn!("*** RUNNING WITH FAKE BACKEND (MOCK MODE ENABLED) ***");
        warn!("*** D-Bus / Docker interactions are mocked        ***");
        warn!("*** NOT FOR PRODUCTION USE                        ***");
        warn!("=====================================================");
        Arc::new(FakeResourceCollector::new())
    } else {
        match CompositeCollector::new(config.resources.clone()).await {
            Ok(c) => Arc::new(c),
            Err(e) => {
                warn!("Failed to initialize native CompositeCollector ({e}); falling back to FakeResourceCollector");
                Arc::new(FakeResourceCollector::new())
            }
        }
    };

    let app = create_app(pool, config.clone(), collector);

    let bind_addr: SocketAddr = config.server.bind.parse().map_err(|e| {
        AppError::Config(format!(
            "Invalid bind address '{}': {e}",
            config.server.bind
        ))
    })?;

    info!("Starting mop server on http://{}", bind_addr);
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to bind to {bind_addr}: {e}")))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| AppError::Internal(format!("Server error: {e}")))?;

    Ok(())
}
