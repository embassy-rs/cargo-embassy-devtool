use petgraph::graph::{Graph, NodeIndex};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ParsedCrate {
    pub package: ParsedPackage,
    pub dependencies: BTreeMap<String, toml::Value>,
    #[serde(rename = "build-dependencies")]
    pub build_dependencies: Option<BTreeMap<String, toml::Value>>,
    #[serde(rename = "dev-dependencies")]
    pub dev_dependencies: Option<BTreeMap<String, toml::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct ParsedPackage {
    pub name: String,
    pub version: String,
    #[serde(default = "default_publish")]
    pub publish: bool,
    #[serde(default)]
    pub metadata: Metadata,
}

fn default_publish() -> bool {
    true
}

#[derive(Debug, Deserialize, Default)]
pub struct Metadata {
    #[serde(default)]
    pub embassy: MetadataEmbassy,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
pub struct MetadataEmbassy {
    #[serde(default)]
    pub skip: bool,
    #[serde(default)]
    pub build: Vec<BuildConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct BuildConfig {
    pub group: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
    pub target: Option<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default, rename = "build-std")]
    pub build_std: Vec<String>,
    #[serde(rename = "artifact-dir")]
    pub artifact_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BuildConfigBatch {
    pub env: std::collections::BTreeMap<String, String>,
    pub build_std: Vec<String>,
}

pub type CrateId = String;

#[derive(Debug, Clone)]
pub struct Crate {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub dependencies: Vec<CrateId>,
    pub build_dependencies: Vec<CrateId>,
    pub dev_dependencies: Vec<CrateId>,
    pub configs: Vec<BuildConfig>,
    pub publish: bool,
}

#[derive(Debug)]
pub struct Context {
    pub root: PathBuf,
    pub crates: BTreeMap<String, Crate>,
    pub graph: GraphContext,
    pub dev_graph: GraphContext,
    pub build_graph: GraphContext,
}

#[derive(Debug)]
pub struct GraphContext {
    pub g: Graph<String, ()>,
    pub i: HashMap<String, NodeIndex>,
}
