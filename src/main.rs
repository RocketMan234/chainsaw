mod cache;
mod graph;
mod parser;
mod query;
mod report;
mod resolver;
mod walker;

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chainsaw", about = "TypeScript/JavaScript dependency graph analyzer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Trace the transitive import weight from an entry point
    Trace {
        /// Entry point file to trace from
        entry: PathBuf,

        /// Compare against another entry point
        #[arg(long)]
        diff: Option<PathBuf>,

        /// Only follow static edges (default behavior)
        #[arg(long, default_value_t = true)]
        static_only: bool,

        /// Also traverse dynamic imports
        #[arg(long)]
        include_dynamic: bool,

        /// Show top N heaviest dependencies
        #[arg(long, default_value_t = 10)]
        top: usize,

        /// Show the full shortest chain to a specific package
        #[arg(long)]
        chain: Option<String>,

        /// Show ALL shortest import chains to a specific package
        #[arg(long)]
        why: Option<String>,

        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,

        /// Force full re-parse, ignoring cache
        #[arg(long)]
        no_cache: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Trace {
            entry,
            diff,
            include_dynamic,
            top,
            chain,
            why,
            json,
            no_cache,
            ..
        } => {
            let start = Instant::now();

            // Determine project root from entry file
            let entry = entry.canonicalize().unwrap_or_else(|e| {
                eprintln!("error: cannot find entry file '{}': {e}", entry.display());
                std::process::exit(1);
            });

            let root = find_project_root(&entry).unwrap_or_else(|| {
                entry.parent().unwrap_or(&entry).to_path_buf()
            });

            // Load or build graph
            let graph = if no_cache {
                let g = walker::build_graph(&root);
                cache::save_cache(&root, &g);
                g
            } else {
                match cache::load_cache(&root) {
                    Some(g) => {
                        eprintln!(
                            "Loaded cached graph ({} modules) in {:.1}ms",
                            g.module_count(),
                            start.elapsed().as_secs_f64() * 1000.0
                        );
                        g
                    }
                    None => {
                        let g = walker::build_graph(&root);
                        eprintln!(
                            "Built graph ({} modules) in {:.1}ms",
                            g.module_count(),
                            start.elapsed().as_secs_f64() * 1000.0
                        );
                        cache::save_cache(&root, &g);
                        g
                    }
                }
            };

            // Resolve entry module ID
            let entry_id = match graph.path_to_id.get(&entry) {
                Some(&id) => id,
                None => {
                    eprintln!(
                        "error: entry file '{}' not found in graph",
                        entry.display()
                    );
                    eprintln!("hint: is it a .ts/.js/.tsx/.jsx file within the project?");
                    std::process::exit(1);
                }
            };

            // Handle --why mode
            if let Some(ref package_name) = why {
                let chains = query::find_all_chains(&graph, entry_id, package_name);
                if json {
                    report::print_why_json(&graph, &chains, package_name, &root);
                } else {
                    report::print_why(&graph, &chains, package_name, &root);
                }
                return;
            }

            // Handle --chain mode
            if let Some(ref package_name) = chain {
                let chain_result = query::find_chain(&graph, entry_id, package_name);
                report::print_chain(&graph, &chain_result, &root);
                return;
            }

            let opts = query::TraceOptions {
                include_dynamic,
                top_n: top,
            };

            let result = query::trace(&graph, entry_id, &opts);

            // Handle --diff mode
            if let Some(diff_path) = diff {
                let diff_entry = diff_path.canonicalize().unwrap_or_else(|e| {
                    eprintln!(
                        "error: cannot find diff entry file '{}': {e}",
                        diff_path.display()
                    );
                    std::process::exit(1);
                });
                let diff_id = match graph.path_to_id.get(&diff_entry) {
                    Some(&id) => id,
                    None => {
                        eprintln!(
                            "error: diff entry file '{}' not found in graph",
                            diff_entry.display()
                        );
                        std::process::exit(1);
                    }
                };
                let diff_result = query::trace(&graph, diff_id, &opts);
                let diff_output = query::diff_traces(&result, &diff_result);

                let entry_rel = entry
                    .strip_prefix(&root)
                    .unwrap_or(&entry)
                    .to_string_lossy();
                let diff_rel = diff_entry
                    .strip_prefix(&root)
                    .unwrap_or(&diff_entry)
                    .to_string_lossy();
                report::print_diff(&diff_output, &entry_rel, &diff_rel);
                return;
            }

            // Normal trace output
            if json {
                report::print_trace_json(&graph, &result, &entry, &root);
            } else {
                report::print_trace(&graph, &result, &entry, &root);
            }

            let elapsed = start.elapsed();
            eprintln!("\nCompleted in {:.1}ms", elapsed.as_secs_f64() * 1000.0);
        }
    }
}

/// Walk up from the entry file to find the project root (directory containing package.json).
fn find_project_root(entry: &PathBuf) -> Option<PathBuf> {
    let mut dir = entry.parent()?;
    loop {
        if dir.join("package.json").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}
