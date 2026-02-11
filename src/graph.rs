use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EdgeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Static,
    Dynamic,
    TypeOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub id: ModuleId,
    pub path: PathBuf,
    pub size_bytes: u64,
    /// None for source files, Some("package-name") for node_modules
    pub package: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub from: ModuleId,
    pub to: ModuleId,
    pub kind: EdgeKind,
    /// The raw import specifier (e.g. "./foo", "@aws-sdk/client-bedrock")
    pub specifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub entry_module: ModuleId,
    pub total_reachable_size: u64,
    pub total_reachable_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleGraph {
    pub modules: Vec<Module>,
    pub edges: Vec<Edge>,
    /// Outgoing edges per module (indexed by ModuleId)
    pub forward_adj: Vec<Vec<EdgeId>>,
    pub path_to_id: HashMap<PathBuf, ModuleId>,
    pub package_map: HashMap<String, PackageInfo>,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
            edges: Vec::new(),
            forward_adj: Vec::new(),
            path_to_id: HashMap::new(),
            package_map: HashMap::new(),
        }
    }

    pub fn add_module(&mut self, path: PathBuf, size_bytes: u64, package: Option<String>) -> ModuleId {
        if let Some(&id) = self.path_to_id.get(&path) {
            return id;
        }
        let id = ModuleId(self.modules.len() as u32);
        self.modules.push(Module {
            id,
            path: path.clone(),
            size_bytes,
            package,
        });
        self.forward_adj.push(Vec::new());
        self.path_to_id.insert(path, id);
        id
    }

    pub fn add_edge(&mut self, from: ModuleId, to: ModuleId, kind: EdgeKind, specifier: String) -> EdgeId {
        let id = EdgeId(self.edges.len() as u32);
        self.edges.push(Edge {
            id,
            from,
            to,
            kind,
            specifier,
        });
        self.forward_adj[from.0 as usize].push(id);
        id
    }

    pub fn module(&self, id: ModuleId) -> &Module {
        &self.modules[id.0 as usize]
    }

    pub fn edge(&self, id: EdgeId) -> &Edge {
        &self.edges[id.0 as usize]
    }

    pub fn outgoing_edges(&self, id: ModuleId) -> &[EdgeId] {
        &self.forward_adj[id.0 as usize]
    }

    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
}
