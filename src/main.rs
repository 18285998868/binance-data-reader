//! Command-line interface: JSON on stdout, logs on stderr.
//!
//! Exit codes: `0` success, `2` invalid request/JSON, `1` I/O or data failure.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use binance_data_reader::{
    CatalogFilter, Config, Dataset, Error, LoadReport, Table, catalog, read,
};

#[derive(Debug, Parser)]
#[command(
    name = "binance-data-reader",
    version,
    about = "Read Binance public archive datasets into JSON columns"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Read one symbol x dataset and print {dataset, columns, report}.
    Read(ReadArgs),
    /// List archive files available below a root.
    List(ListArgs),
}

#[derive(Debug, Args)]
struct ReadArgs {
    /// Path to the TOML request file.
    #[arg(short, long)]
    config: PathBuf,
}

#[derive(Debug, Args)]
struct ListArgs {
    /// Archive root (the downloader's output_dir).
    #[arg(long)]
    root: PathBuf,
    /// Filter by market: spot, futures/um or futures/cm.
    #[arg(long)]
    market: Option<String>,
    /// Filter by cadence: daily or monthly.
    #[arg(long)]
    period: Option<String>,
    /// Filter by dataset name, e.g. klines or metrics.
    #[arg(long)]
    dataset: Option<String>,
    /// Filter by exact symbol.
    #[arg(long)]
    symbol: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReadOutput<'a> {
    dataset: Dataset,
    columns: &'a Table,
    report: &'a LoadReport,
}

fn main() -> std::process::ExitCode {
    init_logging();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            print_error(&error);
            std::process::ExitCode::from(exit_code(&error))
        }
    }
}

fn run() -> Result<(), Error> {
    match Cli::parse().command {
        Command::Read(args) => {
            let config = Config::from_toml_file(&args.config)?;
            let data = read(&config)?;
            let output = ReadOutput {
                dataset: config.dataset(),
                columns: &data.table,
                report: &data.report,
            };
            write_json(&output)
        }
        Command::List(args) => {
            let filter = CatalogFilter {
                market: parse_optional(args.market, "market")?,
                period: parse_optional(args.period, "period")?,
                dataset: parse_optional(args.dataset, "dataset")?,
                symbol: parse_optional(args.symbol, "symbol")?,
            };
            let catalog = catalog(&args.root, &filter)?;
            write_json(&catalog)
        }
    }
}

fn parse_optional<T>(value: Option<String>, field: &str) -> Result<Option<T>, Error>
where
    T: std::str::FromStr<Err = String>,
{
    value
        .map(|raw| {
            raw.parse::<T>()
                .map_err(|e| Error::Config(format!("{field}: {e}")))
        })
        .transpose()
}

fn write_json<T: Serialize>(value: &T) -> Result<(), Error> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, value)?;
    writeln!(lock).map_err(|e| Error::io("write stdout", e))
}

fn exit_code(error: &Error) -> u8 {
    match error {
        Error::Config(_) | Error::Json(_) => 2,
        _ => 1,
    }
}

fn print_error(error: &Error) {
    eprintln!("error: {error}");
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // A host application may already have installed a subscriber; in that
    // case keep it instead of panicking.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
