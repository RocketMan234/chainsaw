use std::path::Path;

use serde::Serialize;

use crate::graph::ModuleGraph;
use crate::query::{DiffResult, TraceResult};

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

pub fn print_trace(graph: &ModuleGraph, result: &TraceResult, entry_path: &Path, root: &Path) {
    println!("{}", relative_path(entry_path, root));
    println!(
        "Static transitive weight: {} ({} modules)",
        format_size(result.static_weight),
        result.static_module_count
    );
    if result.dynamic_only_module_count > 0 {
        println!(
            "Dynamic-only weight: {} ({} modules, not loaded at startup)",
            format_size(result.dynamic_only_weight),
            result.dynamic_only_module_count
        );
    }
    println!();

    if !result.heavy_packages.is_empty() {
        println!("Heavy dependencies (static):");
        for pkg in &result.heavy_packages {
            println!(
                "  {:<35} {}  {} files",
                pkg.name,
                format_size(pkg.total_size),
                pkg.file_count
            );
            if pkg.chain.len() > 1 {
                let chain_str: Vec<String> = pkg
                    .chain
                    .iter()
                    .map(|&mid| {
                        let m = graph.module(mid);
                        if let Some(ref pkg_name) = m.package {
                            pkg_name.clone()
                        } else {
                            relative_path(&m.path, root)
                        }
                    })
                    .collect();
                println!("    -> {}", chain_str.join(" -> "));
            }
        }
        println!();
    }

    if !result.modules_by_cost.is_empty() {
        println!("Modules (sorted by transitive cost):");
        let display_count = result.modules_by_cost.len().min(20);
        for mc in &result.modules_by_cost[..display_count] {
            let m = graph.module(mc.module_id);
            println!(
                "  {:<55} {}",
                relative_path(&m.path, root),
                format_size(mc.transitive_size)
            );
        }
        if result.modules_by_cost.len() > display_count {
            println!(
                "  ... and {} more modules",
                result.modules_by_cost.len() - display_count
            );
        }
    }
}

pub fn print_chain(graph: &ModuleGraph, chain: &[crate::graph::ModuleId], root: &Path) {
    if chain.is_empty() {
        println!("No chain found.");
        return;
    }
    println!("Chain ({} hops):", chain.len() - 1);
    for (i, &mid) in chain.iter().enumerate() {
        let m = graph.module(mid);
        let display = if let Some(ref pkg) = m.package {
            pkg.clone()
        } else {
            relative_path(&m.path, root)
        };
        if i == 0 {
            print!("  {display}");
        } else {
            print!(" -> {display}");
        }
    }
    println!();
}

pub fn print_diff(diff: &DiffResult, entry_a: &str, entry_b: &str) {
    println!("Diff: {entry_a} vs {entry_b}");
    println!();
    println!(
        "  {:<40} {}",
        entry_a,
        format_size(diff.entry_a_weight)
    );
    println!(
        "  {:<40} {}",
        entry_b,
        format_size(diff.entry_b_weight)
    );
    let sign = if diff.weight_delta >= 0 { "+" } else { "" };
    println!(
        "  {:<40} {sign}{}",
        "Delta",
        format_size(diff.weight_delta.unsigned_abs())
    );
    println!();

    if !diff.only_in_a.is_empty() {
        println!("Only in {entry_a}:");
        for pkg in &diff.only_in_a {
            println!("  - {pkg}");
        }
    }
    if !diff.only_in_b.is_empty() {
        println!("Only in {entry_b}:");
        for pkg in &diff.only_in_b {
            println!("  + {pkg}");
        }
    }
    if !diff.shared_packages.is_empty() {
        println!("Shared:");
        for pkg in &diff.shared_packages {
            println!("    {pkg}");
        }
    }
}

pub fn print_why(
    graph: &ModuleGraph,
    chains: &[Vec<crate::graph::ModuleId>],
    package_name: &str,
    root: &Path,
) {
    if chains.is_empty() {
        println!("No chains found to \"{package_name}\".");
        return;
    }
    let hops = chains[0].len().saturating_sub(1);
    println!(
        "{} chain{} to \"{}\" ({} hop{}):\n",
        chains.len(),
        if chains.len() == 1 { "" } else { "s" },
        package_name,
        hops,
        if hops == 1 { "" } else { "s" },
    );
    for (i, chain) in chains.iter().enumerate() {
        let chain_str: Vec<String> = chain
            .iter()
            .map(|&mid| {
                let m = graph.module(mid);
                if let Some(ref pkg) = m.package {
                    pkg.clone()
                } else {
                    relative_path(&m.path, root)
                }
            })
            .collect();
        println!("  {}. {}", i + 1, chain_str.join(" -> "));
    }
}

pub fn print_why_json(
    graph: &ModuleGraph,
    chains: &[Vec<crate::graph::ModuleId>],
    package_name: &str,
    root: &Path,
) {
    let json = JsonWhy {
        package: package_name.to_string(),
        chain_count: chains.len(),
        hop_count: chains.first().map(|c| c.len().saturating_sub(1)).unwrap_or(0),
        chains: chains
            .iter()
            .map(|chain| {
                chain
                    .iter()
                    .map(|&mid| {
                        let m = graph.module(mid);
                        if let Some(ref pkg) = m.package {
                            pkg.clone()
                        } else {
                            relative_path(&m.path, root)
                        }
                    })
                    .collect()
            })
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&json).unwrap());
}

// JSON output types

#[derive(Serialize)]
struct JsonWhy {
    package: String,
    chain_count: usize,
    hop_count: usize,
    chains: Vec<Vec<String>>,
}

#[derive(Serialize)]
struct JsonTrace {
    entry: String,
    static_weight_bytes: u64,
    static_module_count: usize,
    dynamic_only_weight_bytes: u64,
    dynamic_only_module_count: usize,
    heavy_packages: Vec<JsonPackage>,
    modules_by_cost: Vec<JsonModuleCost>,
}

#[derive(Serialize)]
struct JsonPackage {
    name: String,
    total_size_bytes: u64,
    file_count: u32,
    chain: Vec<String>,
}

#[derive(Serialize)]
struct JsonModuleCost {
    path: String,
    transitive_size_bytes: u64,
}

pub fn print_trace_json(
    graph: &ModuleGraph,
    result: &TraceResult,
    entry_path: &Path,
    root: &Path,
) {
    let json = JsonTrace {
        entry: relative_path(entry_path, root),
        static_weight_bytes: result.static_weight,
        static_module_count: result.static_module_count,
        dynamic_only_weight_bytes: result.dynamic_only_weight,
        dynamic_only_module_count: result.dynamic_only_module_count,
        heavy_packages: result
            .heavy_packages
            .iter()
            .map(|pkg| JsonPackage {
                name: pkg.name.clone(),
                total_size_bytes: pkg.total_size,
                file_count: pkg.file_count,
                chain: pkg
                    .chain
                    .iter()
                    .map(|&mid| {
                        let m = graph.module(mid);
                        if let Some(ref pkg_name) = m.package {
                            pkg_name.clone()
                        } else {
                            relative_path(&m.path, root)
                        }
                    })
                    .collect(),
            })
            .collect(),
        modules_by_cost: result
            .modules_by_cost
            .iter()
            .map(|mc| {
                let m = graph.module(mc.module_id);
                JsonModuleCost {
                    path: relative_path(&m.path, root),
                    transitive_size_bytes: mc.transitive_size,
                }
            })
            .collect(),
    };

    println!("{}", serde_json::to_string_pretty(&json).unwrap());
}
