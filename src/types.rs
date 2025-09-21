use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ParsedCrate {
    pub package: ParsedPackage,
    #[serde(default)]
    pub dependencies: BTreeMap<String, toml::Value>,
    #[serde(rename = "build-dependencies", default)]
    pub build_dependencies: BTreeMap<String, toml::Value>,
    #[serde(rename = "dev-dependencies", default)]
    pub dev_dependencies: BTreeMap<String, toml::Value>,
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

pub type CrateId = String;

#[derive(Debug, Clone)]
pub struct Crate {
    pub name: CrateId,
    pub version: String,
    pub path: PathBuf,
    pub dependencies: Vec<CrateId>,
    pub build_dependencies: Vec<CrateId>,
    pub dev_dependencies: Vec<CrateId>,
    pub configs: Vec<BuildConfig>,
    pub publish: bool,
}

impl Crate {
    pub fn all_dependencies(&self) -> impl Iterator<Item = &CrateId> {
        self.dependencies
            .iter()
            .chain(self.build_dependencies.iter())
            .chain(self.dev_dependencies.iter())
    }
}

#[derive(Debug)]
pub struct Context {
    pub root: PathBuf,
    pub crates: BTreeMap<CrateId, Crate>,
    pub reverse_deps: HashMap<CrateId, HashSet<CrateId>>,
}

impl Context {
    pub fn recursive_dependencies(
        &self,
        crates: impl Iterator<Item = impl AsRef<str>>,
    ) -> impl Iterator<Item = CrateId> {
        let mut visited = HashSet::new();
        let mut stack = Vec::new();

        // Initialize stack with input crates
        for crate_name in crates {
            let crate_name = crate_name.as_ref().to_string();
            if !visited.contains(&crate_name) {
                stack.push(crate_name.clone());
                visited.insert(crate_name.clone());
            }
        }

        while let Some(crate_name) = stack.pop() {
            if let Some(krate) = self.crates.get(&crate_name) {
                for dep in krate.all_dependencies() {
                    if !visited.contains(dep) {
                        stack.push(dep.clone());
                        visited.insert(dep.clone());
                    }
                }
            }
        }

        visited.into_iter()
    }

    pub fn recursive_dependents<'a>(
        &self,
        crates: impl Iterator<Item = impl AsRef<str>>,
    ) -> impl Iterator<Item = CrateId> {
        let mut visited = HashSet::new();
        let mut stack = Vec::new();

        // Initialize stack with input crates
        for crate_name in crates {
            let crate_name = crate_name.as_ref().to_string();
            if !visited.contains(&crate_name) {
                stack.push(crate_name.clone());
                visited.insert(crate_name.clone());
            }
        }

        while let Some(crate_name) = stack.pop() {
            if let Some(dependents) = self.reverse_deps.get(&crate_name) {
                for dependent in dependents {
                    if !visited.contains(dependent) {
                        stack.push(dependent.clone());
                        visited.insert(dependent.clone());
                    }
                }
            }
        }

        visited.into_iter()
    }

    pub fn topological_sort(&self) -> Vec<CrateId> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();

        // Visit all crates
        for crate_name in self.crates.keys() {
            self.topological_sort_visit(crate_name, &mut visited, &mut result);
        }

        result
    }

    fn topological_sort_visit(
        &self,
        crate_name: &str,
        visited: &mut HashSet<CrateId>,
        result: &mut Vec<CrateId>,
    ) {
        if visited.contains(crate_name) {
            return;
        }

        visited.insert(crate_name.to_string());

        // Visit all dependencies first
        if let Some(krate) = self.crates.get(crate_name) {
            for dep in krate.all_dependencies() {
                // Only visit dependencies that exist in our crate set
                if self.crates.contains_key(dep) {
                    self.topological_sort_visit(dep, visited, result);
                }
            }
        }

        // Add current crate after all its dependencies
        result.push(crate_name.to_string());
    }
}
