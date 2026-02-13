use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use heapsnap::{analysis, cancel, error, output, parser, serve};

#[derive(Parser, Debug)]
#[command(name = "heapsnap", version, about = "HeapSnapshot CLI Analyzer")]
struct Cli {
    /// Verbose logging (may include object names and strings)
    #[arg(long, default_value_t = false)]
    verbose: bool,

    /// Progress output (default: true). Use --progress=false to disable.
    #[arg(long, default_value_t = true)]
    progress: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Summary(SummaryArgs),
    Retainers(RetainersArgs),
    Build(BuildArgs),
    Diff(DiffArgs),
    Dominator(DominatorArgs),
    Detail(DetailArgs),
    Serve(ServeArgs),
}

#[derive(Args, Debug)]
struct SummaryArgs {
    /// Path to .heapsnapshot
    file: PathBuf,

    /// Show top N constructors
    #[arg(long, default_value_t = 50)]
    top: usize,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Md)]
    format: OutputFormat,

    /// Write JSON output to a file (same as --format json with a path)
    #[arg(long)]
    json: Option<PathBuf>,

    /// Only include constructors containing this string
    #[arg(long = "search", alias = "contains")]
    search: Option<String>,
}

#[derive(Args, Debug)]
struct RetainersArgs {
    /// Path to .heapsnapshot
    file: PathBuf,

    /// Target node id
    #[arg(long)]
    id: Option<u64>,

    /// Target constructor name
    #[arg(long)]
    name: Option<String>,

    /// Pick strategy when multiple targets match --name
    #[arg(long, value_enum, default_value_t = PickStrategy::Largest)]
    pick: PickStrategy,

    /// Max number of paths to output
    #[arg(long, default_value_t = 5)]
    paths: usize,

    /// Max BFS depth
    #[arg(long = "max-depth", default_value_t = 10)]
    max_depth: usize,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Md)]
    format: OutputFormat,
}

#[derive(Args, Debug)]
struct BuildArgs {
    /// Path to .heapsnapshot
    file: PathBuf,

    /// Output directory
    #[arg(long)]
    outdir: PathBuf,

    /// Show top N constructors
    #[arg(long, default_value_t = 50)]
    top: usize,

    /// Only include constructors containing this string
    #[arg(long)]
    contains: Option<String>,
}

#[derive(Args, Debug)]
struct DiffArgs {
    /// Snapshot A
    file_a: PathBuf,

    /// Snapshot B
    file_b: PathBuf,

    /// Show top N constructors
    #[arg(long, default_value_t = 50)]
    top: usize,

    /// Only include constructors containing this string
    #[arg(long)]
    contains: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Md)]
    format: OutputFormat,
}

#[derive(Args, Debug)]
struct DominatorArgs {
    /// Path to .heapsnapshot
    file: PathBuf,

    /// Target node id
    #[arg(long)]
    id: Option<u64>,

    /// Target constructor name
    #[arg(long)]
    name: Option<String>,

    /// Pick strategy when multiple targets match --name
    #[arg(long, value_enum, default_value_t = PickStrategy::Largest)]
    pick: PickStrategy,

    /// Max dominator depth
    #[arg(long = "max-depth", default_value_t = 50)]
    max_depth: usize,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Md)]
    format: OutputFormat,
}

#[derive(Args, Debug)]
struct DetailArgs {
    /// Path to .heapsnapshot
    file: PathBuf,

    /// Target node id
    #[arg(long)]
    id: Option<u64>,

    /// Target constructor name
    #[arg(long)]
    name: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Md)]
    format: OutputFormat,

    /// Skip first N ids in the name list
    #[arg(long, default_value_t = 0)]
    skip: usize,

    /// Limit ids listed for --name or --id constructor summary
    #[arg(long, default_value_t = 200)]
    limit: usize,

    /// Top N retainers (id mode)
    #[arg(long = "top-retainers", default_value_t = 10)]
    top_retainers: usize,

    /// Top N outgoing edges (id mode)
    #[arg(long = "top-edges", default_value_t = 10)]
    top_edges: usize,
}

#[derive(Args, Debug)]
struct ServeArgs {
    /// Path to .heapsnapshot (default file for summary/detail/retainers/dominator)
    file: PathBuf,

    /// Bind address (must be loopback)
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Port
    #[arg(long, default_value_t = 7878)]
    port: u16,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Md,
    Json,
    Csv,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PickStrategy {
    Largest,
    Count,
}

// NOTE: External network access is prohibited. Localhost-only server is allowed.
fn main() {
    let cli = Cli::parse();
    let _cancel = match cancel::install_ctrlc_handler() {
        Ok(token) => token,
        Err(err) => {
            eprintln!("failed to install Ctrl-C handler: {err}");
            cancel::CancelToken::new()
        }
    };

    if let Err(err) = run(cli, _cancel) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run(cli: Cli, cancel: cancel::CancelToken) -> Result<(), error::SnapshotError> {
    match cli.command {
        Command::Summary(args) => run_summary(cli.verbose, cli.progress, cancel, args),
        Command::Retainers(args) => run_retainers(cli.verbose, cli.progress, cancel, args),
        Command::Build(args) => run_build(cli.verbose, cli.progress, cancel, args),
        Command::Diff(args) => run_diff(cli.verbose, cli.progress, cancel, args),
        Command::Dominator(args) => run_dominator(cli.verbose, cli.progress, cancel, args),
        Command::Detail(args) => run_detail(cli.verbose, cli.progress, cancel, args),
        Command::Serve(args) => run_serve(cli.verbose, cli.progress, cancel, args),
    }
}

fn run_serve(
    verbose: bool,
    progress: bool,
    cancel: cancel::CancelToken,
    args: ServeArgs,
) -> Result<(), error::SnapshotError> {
    if args.bind != "127.0.0.1" && args.bind != "localhost" {
        return Err(error::SnapshotError::InvalidData {
            details: "serve only supports loopback bind (use --bind 127.0.0.1)".to_string(),
        });
    }

    if verbose {
        eprintln!(
            "starting local server: file={}, bind={}, port={}",
            args.file.display(),
            args.bind,
            args.port
        );
    }

    serve::run(serve::ServeOptions {
        file: args.file,
        bind: "127.0.0.1".to_string(),
        port: args.port,
        progress,
        cancel,
    })
}

fn run_summary(
    verbose: bool,
    progress: bool,
    cancel: cancel::CancelToken,
    args: SummaryArgs,
) -> Result<(), error::SnapshotError> {
    let started = std::time::Instant::now();
    let options = parser::ReadOptions::new(progress, cancel);
    let snapshot = parser::read_snapshot_file(&args.file, options)?;
    let parse_done = std::time::Instant::now();

    if verbose {
        eprintln!(
            "loaded snapshot: nodes={}, edges={}, strings={}",
            snapshot.node_count(),
            snapshot.edge_count(),
            snapshot.strings.len()
        );
        eprintln!(
            "approx memory: {}",
            format_bytes(snapshot.memory_estimate_bytes())
        );
    }

    let summary = analysis::summary::summarize(
        &snapshot,
        analysis::summary::SummaryOptions {
            top: args.top,
            contains: args.search,
        },
    )?;
    let summary_done = std::time::Instant::now();

    let format = if args.json.is_some() {
        OutputFormat::Json
    } else {
        args.format
    };
    let output = match format {
        OutputFormat::Md => output::summary::format_markdown(&summary),
        OutputFormat::Json => output::summary::format_json(&summary)?,
        OutputFormat::Csv => output::summary::format_csv(&summary),
    };
    let output_path = args.json.as_deref();
    output::write::write_or_stdout(output_path, &output)?;

    if verbose {
        let output_done = std::time::Instant::now();
        eprintln!(
            "timing: parse={:?}, summary={:?}, output={:?}",
            parse_done.duration_since(started),
            summary_done.duration_since(parse_done),
            output_done.duration_since(summary_done)
        );
    }
    Ok(())
}

fn run_retainers(
    verbose: bool,
    progress: bool,
    cancel: cancel::CancelToken,
    args: RetainersArgs,
) -> Result<(), error::SnapshotError> {
    let started = std::time::Instant::now();
    if args.id.is_none() && args.name.is_none() {
        return Err(error::SnapshotError::InvalidData {
            details: "either --id or --name must be specified".to_string(),
        });
    }
    if args.id.is_some() && args.name.is_some() {
        return Err(error::SnapshotError::InvalidData {
            details: "use either --id or --name, not both".to_string(),
        });
    }

    let options = parser::ReadOptions::new(progress, cancel.clone());
    let snapshot = parser::read_snapshot_file(&args.file, options)?;
    let parse_done = std::time::Instant::now();

    if verbose {
        eprintln!(
            "loaded snapshot: nodes={}, edges={}, strings={}",
            snapshot.node_count(),
            snapshot.edge_count(),
            snapshot.strings.len()
        );
        eprintln!(
            "approx memory: {}",
            format_bytes(snapshot.memory_estimate_bytes())
        );
    }

    let target = if let Some(node_id) = args.id {
        analysis::retainers::find_target_by_id(&snapshot, node_id)?
    } else {
        let pick = match args.pick {
            PickStrategy::Largest => analysis::retainers::PickStrategy::Largest,
            PickStrategy::Count => analysis::retainers::PickStrategy::Count,
        };
        analysis::retainers::find_target_by_name(
            &snapshot,
            args.name.as_deref().unwrap_or(""),
            pick,
        )?
    };

    let result = analysis::retainers::find_retaining_paths(
        &snapshot,
        target,
        analysis::retainers::RetainersOptions {
            max_paths: args.paths,
            max_depth: args.max_depth,
            cancel,
        },
    )?;
    let search_done = std::time::Instant::now();

    let output = match args.format {
        OutputFormat::Md => output::retainers::format_markdown(&snapshot, &result),
        OutputFormat::Json => output::retainers::format_json(&snapshot, &result)?,
        OutputFormat::Csv => {
            return Err(error::SnapshotError::InvalidData {
                details: "retainers output does not support csv".to_string(),
            });
        }
    };

    output::write::write_or_stdout(None, &output)?;

    if verbose {
        let output_done = std::time::Instant::now();
        eprintln!(
            "timing: parse={:?}, retainers={:?}, output={:?}",
            parse_done.duration_since(started),
            search_done.duration_since(parse_done),
            output_done.duration_since(search_done)
        );
    }
    Ok(())
}

fn run_build(
    verbose: bool,
    progress: bool,
    cancel: cancel::CancelToken,
    args: BuildArgs,
) -> Result<(), error::SnapshotError> {
    let started = std::time::Instant::now();
    let options = parser::ReadOptions::new(progress, cancel);
    let snapshot = parser::read_snapshot_file(&args.file, options)?;
    let parse_done = std::time::Instant::now();

    if verbose {
        eprintln!(
            "loaded snapshot: nodes={}, edges={}, strings={}",
            snapshot.node_count(),
            snapshot.edge_count(),
            snapshot.strings.len()
        );
        eprintln!(
            "approx memory: {}",
            format_bytes(snapshot.memory_estimate_bytes())
        );
    }

    let summary = analysis::summary::summarize(
        &snapshot,
        analysis::summary::SummaryOptions {
            top: args.top,
            contains: args.contains,
        },
    )?;
    let summary_done = std::time::Instant::now();

    std::fs::create_dir_all(&args.outdir).map_err(error::SnapshotError::Io)?;
    let summary_path = args.outdir.join("summary.json");
    let meta_path = args.outdir.join("meta.json");

    let summary_json = output::summary::format_json(&summary)?;
    output::write::write_or_stdout(Some(&summary_path), &summary_json)?;

    let meta = output::build::BuildMeta::from_snapshot(&snapshot);
    let meta_json = meta.to_json()?;
    output::write::write_or_stdout(Some(&meta_path), &meta_json)?;

    if verbose {
        let output_done = std::time::Instant::now();
        eprintln!(
            "timing: parse={:?}, summary={:?}, output={:?}",
            parse_done.duration_since(started),
            summary_done.duration_since(parse_done),
            output_done.duration_since(summary_done)
        );
    }

    Ok(())
}

fn run_diff(
    verbose: bool,
    progress: bool,
    cancel: cancel::CancelToken,
    args: DiffArgs,
) -> Result<(), error::SnapshotError> {
    let started = std::time::Instant::now();
    let options_a = parser::ReadOptions::new(progress, cancel.clone());
    let snapshot_a = parser::read_snapshot_file(&args.file_a, options_a)?;
    let parse_a_done = std::time::Instant::now();

    let options_b = parser::ReadOptions::new(progress, cancel);
    let snapshot_b = parser::read_snapshot_file(&args.file_b, options_b)?;
    let parse_b_done = std::time::Instant::now();

    if verbose {
        eprintln!(
            "loaded snapshots: A nodes={}, B nodes={}",
            snapshot_a.node_count(),
            snapshot_b.node_count()
        );
    }

    let diff = analysis::diff::diff_summaries(
        &snapshot_a,
        &snapshot_b,
        analysis::diff::DiffOptions {
            top: args.top,
            contains: args.contains,
        },
    )?;
    let diff_done = std::time::Instant::now();

    let output = match args.format {
        OutputFormat::Md => output::diff::format_markdown(&diff),
        OutputFormat::Json => output::diff::format_json(&diff)?,
        OutputFormat::Csv => output::diff::format_csv(&diff),
    };
    output::write::write_or_stdout(None, &output)?;

    if verbose {
        let output_done = std::time::Instant::now();
        eprintln!(
            "timing: parse_a={:?}, parse_b={:?}, diff={:?}, output={:?}",
            parse_a_done.duration_since(started),
            parse_b_done.duration_since(parse_a_done),
            diff_done.duration_since(parse_b_done),
            output_done.duration_since(diff_done)
        );
    }

    Ok(())
}

fn run_dominator(
    verbose: bool,
    progress: bool,
    cancel: cancel::CancelToken,
    args: DominatorArgs,
) -> Result<(), error::SnapshotError> {
    if args.id.is_none() && args.name.is_none() {
        return Err(error::SnapshotError::InvalidData {
            details: "either --id or --name must be specified".to_string(),
        });
    }
    if args.id.is_some() && args.name.is_some() {
        return Err(error::SnapshotError::InvalidData {
            details: "use either --id or --name, not both".to_string(),
        });
    }

    let started = std::time::Instant::now();
    let options = parser::ReadOptions::new(progress, cancel.clone());
    let snapshot = parser::read_snapshot_file(&args.file, options)?;
    let parse_done = std::time::Instant::now();

    if verbose {
        eprintln!(
            "loaded snapshot: nodes={}, edges={}, strings={}",
            snapshot.node_count(),
            snapshot.edge_count(),
            snapshot.strings.len()
        );
        eprintln!(
            "approx memory: {}",
            format_bytes(snapshot.memory_estimate_bytes())
        );
    }

    let target = if let Some(node_id) = args.id {
        analysis::retainers::find_target_by_id(&snapshot, node_id)?
    } else {
        let pick = match args.pick {
            PickStrategy::Largest => analysis::retainers::PickStrategy::Largest,
            PickStrategy::Count => analysis::retainers::PickStrategy::Count,
        };
        analysis::retainers::find_target_by_name(
            &snapshot,
            args.name.as_deref().unwrap_or(""),
            pick,
        )?
    };

    let result = analysis::dominator::dominator_chain(
        &snapshot,
        target,
        analysis::dominator::DominatorOptions {
            max_depth: args.max_depth,
            cancel,
        },
    )?;
    let dom_done = std::time::Instant::now();

    let output = match args.format {
        OutputFormat::Md => output::dominator::format_markdown(&snapshot, &result),
        OutputFormat::Json => output::dominator::format_json(&snapshot, &result)?,
        OutputFormat::Csv => {
            return Err(error::SnapshotError::InvalidData {
                details: "dominator output does not support csv".to_string(),
            });
        }
    };

    output::write::write_or_stdout(None, &output)?;

    if verbose {
        let output_done = std::time::Instant::now();
        eprintln!(
            "timing: parse={:?}, dominator={:?}, output={:?}",
            parse_done.duration_since(started),
            dom_done.duration_since(parse_done),
            output_done.duration_since(dom_done)
        );
    }

    Ok(())
}

fn run_detail(
    verbose: bool,
    progress: bool,
    cancel: cancel::CancelToken,
    args: DetailArgs,
) -> Result<(), error::SnapshotError> {
    let started = std::time::Instant::now();
    if args.id.is_none() && args.name.is_none() {
        return Err(error::SnapshotError::InvalidData {
            details: "either --id or --name must be specified".to_string(),
        });
    }
    if args.id.is_some() && args.name.is_some() {
        return Err(error::SnapshotError::InvalidData {
            details: "use either --id or --name, not both".to_string(),
        });
    }

    let options = parser::ReadOptions::new(progress, cancel);
    let snapshot = parser::read_snapshot_file(&args.file, options)?;
    let parse_done = std::time::Instant::now();

    if verbose {
        eprintln!(
            "loaded snapshot: nodes={}, edges={}, strings={}",
            snapshot.node_count(),
            snapshot.edge_count(),
            snapshot.strings.len()
        );
        eprintln!(
            "approx memory: {}",
            format_bytes(snapshot.memory_estimate_bytes())
        );
    }

    let detail = analysis::detail::detail(
        &snapshot,
        analysis::detail::DetailOptions {
            id: args.id,
            name: args.name.clone(),
            skip: args.skip,
            limit: args.limit,
            top_retainers: args.top_retainers,
            top_edges: args.top_edges,
        },
    )?;
    let detail_done = std::time::Instant::now();

    let output = match args.format {
        OutputFormat::Md => output::detail::format_markdown(&detail),
        OutputFormat::Json => output::detail::format_json(&detail)?,
        OutputFormat::Csv => output::detail::format_csv(&detail),
    };
    output::write::write_or_stdout(None, &output)?;

    if verbose {
        let output_done = std::time::Instant::now();
        eprintln!(
            "timing: parse={:?}, detail={:?}, output={:?}",
            parse_done.duration_since(started),
            detail_done.duration_since(parse_done),
            output_done.duration_since(detail_done)
        );
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_parsing_summary() {
        let args = Cli::try_parse_from(["heapsnap", "summary", "input.heapsnapshot"]);
        assert!(args.is_ok());
    }

    #[test]
    fn help_parsing_summary_search() {
        let args = Cli::try_parse_from([
            "heapsnap",
            "summary",
            "input.heapsnapshot",
            "--search",
            "Foo",
        ]);
        assert!(args.is_ok());
    }

    #[test]
    fn help_parsing_retainers() {
        let args =
            Cli::try_parse_from(["heapsnap", "retainers", "input.heapsnapshot", "--id", "123"]);
        assert!(args.is_ok());
    }

    #[test]
    fn help_parsing_build() {
        let args =
            Cli::try_parse_from(["heapsnap", "build", "input.heapsnapshot", "--outdir", "out"]);
        assert!(args.is_ok());
    }

    #[test]
    fn help_parsing_diff() {
        let args = Cli::try_parse_from([
            "heapsnap",
            "diff",
            "a.heapsnapshot",
            "b.heapsnapshot",
            "--format",
            "json",
        ]);
        assert!(args.is_ok());
    }

    #[test]
    fn help_parsing_dominator() {
        let args =
            Cli::try_parse_from(["heapsnap", "dominator", "input.heapsnapshot", "--id", "123"]);
        assert!(args.is_ok());
    }

    #[test]
    fn help_parsing_detail() {
        let args = Cli::try_parse_from(["heapsnap", "detail", "input.heapsnapshot", "--id", "123"]);
        assert!(args.is_ok());
    }

    #[test]
    fn help_parsing_serve() {
        let args =
            Cli::try_parse_from(["heapsnap", "serve", "input.heapsnapshot", "--port", "7878"]);
        assert!(args.is_ok());
    }
}
