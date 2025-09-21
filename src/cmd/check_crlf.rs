use crate::types::Context;
use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, clap::Args)]
/// Check that all files in the repository have LF line endings (no CRLF)
pub struct Args;

pub fn run(ctx: &Context, _args: Args) -> Result<()> {
    let mut files_with_crlf = Vec::new();
    
    // Walk through all files in the repository
    for entry in WalkDir::new(&ctx.root)
        .into_iter()
        .filter_entry(|e| !is_ignored_path(e.path()))
    {
        let entry = entry?;
        let path = entry.path();
        
        // Only check regular files
        if !path.is_file() {
            continue;
        }
        
        // Skip binary files by checking if they're likely text files
        if !is_likely_text_file(path) {
            continue;
        }
        
        // Read file as bytes to detect CRLF
        match fs::read(path) {
            Ok(contents) => {
                if contains_crlf(&contents) {
                    let relative_path = path.strip_prefix(&ctx.root)
                        .unwrap_or(path)
                        .display()
                        .to_string();
                    files_with_crlf.push(relative_path);
                }
            }
            Err(e) => {
                // Skip files we can't read (permissions, etc.)
                eprintln!("Warning: Could not read {}: {}", path.display(), e);
            }
        }
    }
    
    if files_with_crlf.is_empty() {
        println!("✅ All text files have LF line endings!");
        Ok(())
    } else {
        for file in &files_with_crlf {
            eprintln!("❌ File has CRLF line endings: {}", file);
        }
        Err(anyhow!("Found {} files with CRLF line endings", files_with_crlf.len()))
    }
}

fn is_ignored_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    
    // Skip common directories and files that should be ignored
    path_str.contains("/.git/") ||
    path_str.contains("/target/") ||
    path_str.contains("/node_modules/") ||
    path_str.contains("/.cargo/") ||
    path_str.ends_with("/.DS_Store") ||
    path_str.ends_with("/Thumbs.db") ||
    path_str.contains("/__pycache__/") ||
    path_str.contains("/.pytest_cache/")
}

fn is_likely_text_file(path: &Path) -> bool {
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        // Common text file extensions
        matches!(extension.to_lowercase().as_str(),
            "rs" | "toml" | "md" | "txt" | "yml" | "yaml" | "json" | 
            "js" | "ts" | "html" | "css" | "scss" | "xml" | "svg" |
            "py" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" |
            "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" | "hxx" |
            "java" | "kt" | "scala" | "go" | "rb" | "php" | "cs" |
            "gitignore" | "gitattributes" | "dockerignore" | "dockerfile" |
            "makefile" | "cmake" | "gradle" | "sbt" | "pom" |
            "log" | "ini" | "cfg" | "conf" | "config"
        )
    } else {
        // Files without extensions - check if they're known text files
        if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
            matches!(filename.to_lowercase().as_str(),
                "readme" | "license" | "changelog" | "authors" | "contributors" |
                "dockerfile" | "makefile" | "rakefile" | "gemfile" | "pipfile" |
                "cargo.lock" | "package.json" | "package-lock.json" |
                "yarn.lock" | "pnpm-lock.yaml" | ".gitignore" | ".gitattributes" |
                ".dockerignore" | ".editorconfig" | ".rustfmt.toml"
            )
        } else {
            false
        }
    }
}

fn contains_crlf(contents: &[u8]) -> bool {
    // Look for CRLF sequences (\r\n)
    contents.windows(2).any(|window| window == b"\r\n")
}