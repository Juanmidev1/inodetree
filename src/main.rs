use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::Ordering::Relaxed;

use clap::Parser;
use inodetree::arena::{Arena, NodeId, ROOT};
use inodetree::format::format_size;
use inodetree::scan::{ScanConfig, ScanResult, vfs};
use inodetree::stats;

#[derive(Parser)]
#[command(version, about = "Fast Linux disk space analyzer")]
struct Cli {
    /// Directory to scan
    path: Option<PathBuf>,
    /// Print a text report instead of launching the GUI
    #[arg(long)]
    headless: bool,
    /// Number of rows in each top list
    #[arg(long, default_value_t = 10)]
    top: usize,
    /// Show logical size (`stx_size`) in the top lists instead of allocated disk usage
    #[arg(long)]
    logical: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.headless {
        return run_headless(&cli);
    }
    match inodetree::ui::run(cli.path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Cannot start the GUI: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_headless(cli: &Cli) -> ExitCode {
    let Some(path) = cli.path.as_ref() else {
        eprintln!("--headless requires a PATH to scan");
        return ExitCode::FAILURE;
    };
    let config = ScanConfig::new(path);
    let result = match vfs::scan(&config) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Cannot scan {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let unreadable_dirs = config.progress.unreadable_dirs.load(Relaxed);
    // A closed pipe (for example `| head`) is a normal way to stop reading, not a failure
    match print_report(&mut io::stdout().lock(), &result, cli, unreadable_dirs) {
        Err(error) if error.kind() != io::ErrorKind::BrokenPipe => {
            eprintln!("Cannot write report: {error}");
            ExitCode::FAILURE
        }
        _ => ExitCode::SUCCESS,
    }
}

fn print_report(
    out: &mut impl Write,
    result: &ScanResult,
    cli: &Cli,
    unreadable_dirs: u64,
) -> io::Result<()> {
    let arena = &result.arena;
    let root = &arena.nodes[ROOT as usize];
    writeln!(
        out,
        "Scanned {} files and {} folders in {:.2?}: {} allocated, {} logical",
        root.file_count,
        root.dir_count,
        result.elapsed,
        format_size(root.allocated_size),
        format_size(root.logical_size),
    )?;
    if unreadable_dirs > 0 {
        writeln!(
            out,
            "Warning: {unreadable_dirs} directories could not be read"
        )?;
    }
    let size_of = |id: NodeId| {
        let node = &arena.nodes[id as usize];
        if cli.logical {
            node.logical_size
        } else {
            node.allocated_size
        }
    };
    writeln!(out, "\nLargest folders:")?;
    print_nodes(out, arena, &stats::top_dirs(arena, cli.top), size_of)?;
    writeln!(out, "\nLargest files:")?;
    print_nodes(out, arena, &stats::top_files(arena, cli.top), size_of)?;
    print_extensions(out, arena, cli.top)
}

fn print_nodes(
    out: &mut impl Write,
    arena: &Arena,
    ids: &[NodeId],
    size_of: impl Fn(NodeId) -> u64,
) -> io::Result<()> {
    for &id in ids {
        writeln!(
            out,
            "{:>12}  {}",
            format_size(size_of(id)),
            arena.path_of(id)
        )?;
    }
    Ok(())
}

fn print_extensions(out: &mut impl Write, arena: &Arena, top: usize) -> io::Result<()> {
    writeln!(out, "\nLargest extensions:")?;
    for entry in stats::extension_breakdown(arena).iter().take(top) {
        let name = arena.extensions.name(entry.ext_id);
        let label = if name.is_empty() { "(none)" } else { name };
        let size = format_size(entry.allocated_size);
        writeln!(out, "{size:>12}  {:>9} files  {label}", entry.file_count)?;
    }
    Ok(())
}
