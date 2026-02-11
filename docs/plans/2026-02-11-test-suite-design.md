# Test Suite Design

## Motivation

Every bug filed against chainsaw (#1-#8) was a silent failure -- wrong output with no crash or error. The parser misses an import pattern, the query deduplicates incorrectly, the diff compares truncated data. These failures are invisible without tests.

## Approach

Inline unit tests in `src/parser.rs` and `src/query.rs` using `#[cfg(test)]`. No fixture files, no filesystem I/O. Parser tests use inline TS/JS source strings parsed by SWC. Query tests build small in-memory `ModuleGraph` instances.

## Parser Tests (~15 cases)

Helper: `parse_ts(source: &str) -> Vec<RawImport>` -- parses TypeScript source with SWC and calls `extract_imports`.

Static imports:
- Named import: `import { x } from "y"`
- Default import: `import x from "y"`
- Re-export: `export { x } from "y"`
- Star re-export: `export * from "y"`
- CommonJS: `const x = require("y")`

Type-only (must classify as TypeOnly, not Static):
- Statement-level: `import type { X } from "y"`
- All specifiers type-only: `import { type X, type Y } from "y"`
- Mixed (should be Static): `import { type X, y } from "y"`
- Type re-export: `export type { X } from "y"`

Dynamic imports:
- Standard: `await import("y")`
- Expression-position (bug #8): `import("y").then(cb)`
- Inside arrow: `const load = () => import("y")`
- Nested require in if block

Edge cases:
- Dynamic import in try/catch
- Multiple imports in one file (verify all found with correct kinds)

## Query Tests (~10 cases)

Helper: `make_graph(nodes, edges) -> ModuleGraph` -- builds a graph from a declarative spec of (path, size, package) tuples and (from, to, kind) edges.

BFS / trace:
- Static reachability: verify weight = sum of reachable sizes
- Dynamic edges excluded by default
- include_dynamic flag includes dynamic-only modules

Chain finding:
- Single chain: linear path to package
- Multiple chains with package-level dedup
- No chain exists: empty result

Cut points:
- Single cut point in diamond graph
- No cut point with independent paths
- Single-chain ascending sort (surgical first)

Diff:
- Snapshot diff with overlapping packages: verify only_in_a, only_in_b, shared, delta
