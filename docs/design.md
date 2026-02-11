# Chainsaw Design Document

> For Claude: REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

Goal: A Rust CLI that scans TypeScript/JavaScript codebases, builds a full dependency graph (including node_modules), and reports transitive import weight from any entry point -- showing exactly which heavy dependencies load at startup and the shortest chain to each.

Architecture: Parse all reachable files with SWC in parallel, resolve imports with oxc_resolver, build an in-memory adjacency graph, cache it to disk, then answer queries via BFS traversal in microseconds.

Tech Stack: Rust, swc_ecma_parser, oxc_resolver, rayon, ignore, bitcode, clap

---

## Problem

In large TypeScript projects (e.g. openclaw, ~480K lines), a single static import of a lightweight function can transitively pull in megabytes of heavy dependencies (AWS SDK, AJV, OpenAI SDK, etc.) at module load time. These chains are invisible during development and cause severe cold-start latency.

Current discovery process is manual and slow:
1. Benchmark a command, notice it's slow
2. Use strace on a VPS to trace which node_modules load at runtime
3. Use dependency-cruiser + custom BFS scripts to trace the source-level path
4. Manually identify which imports to defer or extract

Chainsaw automates this entire workflow in milliseconds.

---

## Core Model

Three concepts:

- Module: A .ts/.js/.tsx/.jsx file, or a node_modules package entry point. Has a file size on disk.
- Edge: An import from one module to another. Tagged as static, dynamic, or type-only. Static edges load at startup; dynamic edges are lazy; type-only edges are erased at runtime.
- Graph: All modules + edges. Built once (cached), queried many times.

### Import Classification

| Syntax | Edge Kind | Counts Toward Startup |
|--------|-----------|----------------------|
| `import { x } from "y"` | static | yes |
| `export { x } from "y"` | static | yes (re-exports trigger eager loading) |
| `require("y")` | static | yes |
| `await import("y")` | dynamic | no |
| `import type { x } from "y"` | type-only | no (erased at compile time) |
| `export type { x } from "y"` | type-only | no |

---

## Performance Architecture

Target: Full graph of ~480K lines TS + transitive node_modules in <100ms cold, <1ms cached.

### Layer 1: Parallel File Discovery + Parsing

- Use `ignore` crate (same engine as ripgrep) for filesystem walking. Respects .gitignore, skips irrelevant dirs.
- Parse files in parallel with `rayon` thread pool. SWC parses a typical TS file in ~50-200us. With 8 cores, 2000 source files completes in ~50ms.
- For node_modules: only parse packages that are actually reachable from source. Follow edges lazily -- don't parse all installed packages.

### Layer 2: On-Disk Cache

- After first parse, serialize the graph to a binary cache file (.chainsaw.cache) using bitcode.
- Cache key per file: path + mtime + size. On subsequent runs, only re-parse changed files.
- Warm cache load: read binary blob + verify mtimes = single-digit ms.

### Layer 3: In-Memory Graph Queries

- Computing transitive closure from an entry point is BFS over adjacency lists -- microseconds.
- Weight aggregation (sum file sizes along static edges) is accumulation during traversal.
- Shortest path to heavy dep is standard BFS -- microseconds.

No build step, no bundler, no JavaScript runtime.

---

## CLI Interface

### trace command

```bash
chainsaw trace src/cli/program/config-guard.ts
```

Output:

```
src/cli/program/config-guard.ts
Static transitive weight: 4.8 MB (312 modules)
Dynamic-only weight: 1.2 MB (45 modules, not loaded at startup)

Heavy dependencies (static):
  @aws-sdk/client-bedrock     2.1 MB  240 files
    -> doctor-config-flow.ts -> plugin-auto-enable.ts -> model-selection.ts
       -> models-config.providers.ts -> bedrock-discovery.ts -> @aws-sdk/client-bedrock

  @sinclair/typebox            890 KB  219 files
    -> doctor-config-flow.ts -> config/io.ts -> validation.ts -> ajv -> @sinclair/typebox

  highlight.js                 650 KB  195 files
    -> doctor-config-flow.ts -> config/io.ts -> ... -> highlight.js

  openai                       420 KB  110 files
    -> ... -> models-config.providers.ts -> openai

Modules (sorted by transitive cost):
  src/agents/models-config.providers.ts   3.4 MB  (gateway to all providers)
  src/agents/model-selection.ts           3.4 MB  (imports models-config.providers)
  src/plugins/plugin-auto-enable.ts       3.4 MB  (imports model-selection)
  ...
```

### diff mode

```bash
chainsaw trace src/cli/program/config-guard.ts --diff src/cli/program/preaction.ts
```

Shows weight differences between two entry points.

### Flags

- `--static-only` -- only follow static edges (default)
- `--include-dynamic` -- also traverse dynamic imports
- `--top N` -- show top N heaviest deps (default 10)
- `--chain PACKAGE` -- show the full shortest chain to a specific package
- `--json` -- machine-readable output
- `--no-cache` -- force full re-parse

The "Modules sorted by transitive cost" section answers the key question: "if I made this import dynamic, how much startup weight would I save?"

---

## Module Resolution

Uses `oxc_resolver` (from the OXC project, same ecosystem as oxlint). Full Node module resolution algorithm in Rust, battle-tested, extremely fast.

Handles:
- Relative imports with .js -> .ts extension mapping
- Bare specifiers via node_modules walking
- Package.json exports field (subpath exports, conditions, patterns)
- Package.json main/module fields (legacy fallback)
- Package imports field (self-referencing #internal imports)
- Scoped packages (@aws-sdk/client-bedrock)
- Index file resolution (dir/index.ts)

Skipped (zero runtime cost):
- Built-in Node modules (node:path, node:fs) -- ignored completely
- CSS/JSON/image imports -- count file size but don't parse for further imports

### Package Weight Calculation

For a node_modules package, weight = sum of all files reachable from its entry point via static imports. We attribute the entire reachable weight to the edge that first enters the package.

---

## Data Structures

```rust
struct ModuleGraph {
    modules: Vec<Module>,                    // arena-allocated, indexed by ModuleId
    edges: Vec<Edge>,                        // all edges
    forward_adj: Vec<Vec<EdgeId>>,           // outgoing edges per module
    path_to_id: HashMap<PathBuf, ModuleId>,  // fast lookup by path
    package_map: HashMap<String, PackageInfo>, // package name -> aggregated info
}

struct Module {
    id: ModuleId,
    path: PathBuf,
    size_bytes: u64,
    package: Option<String>,  // None for source files, Some("ajv") for node_modules
}

struct Edge {
    from: ModuleId,
    to: ModuleId,
    kind: EdgeKind,  // Static | Dynamic | TypeOnly
}

struct PackageInfo {
    name: String,
    entry_module: ModuleId,
    total_reachable_size: u64,
    total_reachable_files: u32,
}
```

---

## Pipeline

```
1. CLI parses args (clap)
2. Load cache if valid (.chainsaw.cache)
3. Walk source files (ignore crate, parallel via rayon)
4. Parse each file with SWC, extract imports
5. For each import specifier, resolve with oxc_resolver
6. If resolved target is new and not cached, parse it too (recursive into node_modules)
7. Build adjacency lists
8. Save updated cache
9. Run BFS from entry point, following only static edges
10. Aggregate weights, find shortest chains to heavy packages
11. Print report
```

Steps 3-7 happen in parallel and only for files not in cache. Steps 9-10 are pure in-memory graph traversal.

---

## Project Structure

```
~/dev/RocketMan234/chainsaw/
  Cargo.toml
  src/
    main.rs          -- CLI entry point, clap arg parsing
    graph.rs         -- ModuleGraph, Module, Edge, PackageInfo types
    parser.rs        -- SWC import extraction (static, dynamic, type-only)
    resolver.rs      -- oxc_resolver wrapper, extension mapping
    walker.rs        -- filesystem discovery + parallel parsing pipeline
    cache.rs         -- binary cache read/write with mtime validation
    query.rs         -- BFS traversal, weight aggregation, shortest chain finding
    report.rs        -- output formatting (human-readable + JSON)
```

---

## Crate Dependencies

| Crate | Purpose |
|-------|---------|
| swc_ecma_parser | Parse TS/JS/TSX/JSX, extract import statements |
| swc_ecma_ast | AST types for import/export nodes |
| oxc_resolver | Full Node module resolution |
| rayon | Parallel file parsing |
| ignore | Fast gitignore-aware filesystem walking |
| bitcode | Binary cache serialization/deserialization |
| clap (derive) | CLI argument parsing |
| serde | Serialization for cache and JSON output |
