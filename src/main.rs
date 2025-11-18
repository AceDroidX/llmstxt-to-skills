use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use url::Url;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REGISTRY_FILENAME: &str = ".claude-skills-registry.toml";

/// Registry configuration containing all llms.txt sources
#[derive(Debug, Serialize, Deserialize)]
struct RegistryConfig {
    #[serde(rename = "source")]
    sources: Vec<Source>,
}

/// A single llms.txt source with optional filters
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Source {
    url: String,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

/// Metadata stored with each generated skill
#[derive(Debug, Serialize, Deserialize)]
struct SkillMetadata {
    source_url: String,
    entry_url: String,
    generated_at: DateTime<Utc>,
    generator_version: String,
}

/// Generate Claude Skills from llms.txt documentation files
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// URL to the llms.txt file (standalone mode, bypasses registry)
    #[arg(global = true, required_unless_present = "command")]
    url: Option<String>,

    /// Output directory for generated skills
    #[arg(short, long, default_value = "./skills", global = true)]
    output_dir: PathBuf,

    /// Include only URLs matching this pattern (can be specified multiple times)
    #[arg(long, global = true)]
    include: Vec<String>,

    /// Exclude URLs matching this pattern (can be specified multiple times)
    #[arg(long, global = true)]
    exclude: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new registry file
    Init {
        /// Path to registry file (default: ./.claude-skills-registry.toml)
        #[arg(long)]
        registry: Option<PathBuf>,
    },
    /// Add a source to the registry
    Add {
        /// URL to the llms.txt file
        url: String,

        /// Include only URLs matching this pattern
        #[arg(long)]
        include: Vec<String>,

        /// Exclude URLs matching this pattern
        #[arg(long)]
        exclude: Vec<String>,

        /// Path to registry file (default: ./.claude-skills-registry.toml)
        #[arg(long)]
        registry: Option<PathBuf>,
    },
    /// Update skills from registry sources
    Update {
        /// Only update from this source URL (updates all if not specified)
        #[arg(long)]
        source: Option<String>,

        /// Path to registry file (default: ./.claude-skills-registry.toml)
        #[arg(long)]
        registry: Option<PathBuf>,
    },
    /// List all sources in the registry
    List {
        /// Path to registry file (default: ./.claude-skills-registry.toml)
        #[arg(long)]
        registry: Option<PathBuf>,
    },
}

/// Represents a single entry from an llms.txt file
#[derive(Debug, Clone)]
struct LlmsTxtEntry {
    title: String,
    url: String,
    description: String,
}

impl LlmsTxtEntry {
    fn new(title: String, url: String, description: Option<String>) -> Self {
        Self {
            title,
            url,
            description: description.unwrap_or_default(),
        }
    }
}

/// Generates Claude Skills from llms.txt entries
struct SkillGenerator {
    output_dir: PathBuf,
    verb_conversions: HashMap<&'static str, &'static str>,
    client: reqwest::Client,
}

impl SkillGenerator {
    fn new(output_dir: PathBuf, client: reqwest::Client) -> Result<Self> {
        std::fs::create_dir_all(&output_dir)
            .context(format!("Failed to create output directory: {:?}", output_dir))?;

        let mut verb_conversions = HashMap::new();
        verb_conversions.insert("get", "getting");
        verb_conversions.insert("list", "listing");
        verb_conversions.insert("create", "creating");
        verb_conversions.insert("update", "updating");
        verb_conversions.insert("delete", "deleting");
        verb_conversions.insert("remove", "removing");
        verb_conversions.insert("add", "adding");
        verb_conversions.insert("cancel", "canceling");
        verb_conversions.insert("archive", "archiving");
        verb_conversions.insert("retrieve", "retrieving");
        verb_conversions.insert("send", "sending");
        verb_conversions.insert("download", "downloading");
        verb_conversions.insert("upload", "uploading");
        verb_conversions.insert("count", "counting");
        verb_conversions.insert("generate", "generating");
        verb_conversions.insert("improve", "improving");
        verb_conversions.insert("build", "building");
        verb_conversions.insert("migrate", "migrating");
        verb_conversions.insert("implement", "implementing");
        verb_conversions.insert("optimize", "optimizing");
        verb_conversions.insert("configure", "configuring");
        verb_conversions.insert("install", "installing");
        verb_conversions.insert("deploy", "deploying");

        Ok(Self {
            output_dir,
            verb_conversions,
            client,
        })
    }

    /// Convert a title to gerund form (e.g., 'Get API Key' -> 'Getting API Keys')
    fn title_to_gerund(&self, title: &str) -> String {
        let words: Vec<&str> = title.split_whitespace().collect();

        if words.is_empty() {
            return title.to_string();
        }

        let mut result = Vec::new();
        let first_word = words[0].to_lowercase();

        if let Some(&gerund) = self.verb_conversions.get(first_word.as_str()) {
            result.push(capitalize_first(gerund));
        } else {
            result.push(capitalize_first(&first_word));
        }

        for word in &words[1..] {
            result.push(capitalize_first(&word.to_lowercase()));
        }

        result.join(" ")
    }

    /// Convert a title to a skill directory name (hyphenated, lowercase)
    fn title_to_skill_name(&self, title: &str) -> String {
        let gerund_title = self.title_to_gerund(title);
        let lowercase = gerund_title.to_lowercase();
        let re = Regex::new(r"[^\w\s-]").unwrap();
        let name = re.replace_all(&lowercase, "");
        let re_spaces = Regex::new(r"[-\s]+").unwrap();
        let name = re_spaces.replace_all(&name, "-");
        name.trim_matches('-').to_string()
    }

    /// Create a proper skill description (200-1024 chars, third-person)
    fn create_description(&self, entry: &LlmsTxtEntry) -> String {
        let mut base_desc = if !entry.description.is_empty() {
            entry.description.clone()
        } else {
            format!("Provides guidance and information about {}.", entry.title.to_lowercase())
        };

        if !base_desc.ends_with('.') {
            base_desc.push('.');
        }

        let usage_context = format!(
            " Use when working with {} or when user mentions {}.",
            entry.title.to_lowercase(),
            entry.title.to_lowercase()
        );

        let mut full_desc = format!("{}{}", base_desc, usage_context);

        // Pad if too short
        while full_desc.len() < 200 {
            full_desc.push_str(&format!(
                " This skill includes comprehensive documentation and examples for {}.",
                entry.title.to_lowercase()
            ));
        }

        // Truncate if too long
        if full_desc.len() > 1024 {
            full_desc.truncate(1021);
            full_desc.push_str("...");
        }

        full_desc
    }

    /// Create SKILL.md content with proper frontmatter
    fn create_skill_md(
        &self,
        title: &str,
        description: &str,
        entry: &LlmsTxtEntry,
        has_reference: bool,
    ) -> String {
        let mut content = format!(
            r#"---
name: {}
description: {}
version: 1.0.0
---

# {}

This skill provides guidance and information about {}.

## Quick Start

This skill contains documentation extracted from the official source. "#,
            title,
            description,
            title,
            entry.title.to_lowercase()
        );

        if has_reference {
            let overview = if !entry.description.is_empty() {
                entry.description.clone()
            } else {
                format!("Comprehensive information about {}.", entry.title.to_lowercase())
            };

            content.push_str(&format!(
                r#"For complete details, see [reference.md](reference.md).

## Overview

{}

## How to Use

When you need information about {}:
1. Ask Claude about the specific aspect you need help with
2. Claude will reference the documentation in this skill
3. Follow the guidance provided in the reference documentation

## Reference Documentation

Complete documentation is available in [reference.md](reference.md), which includes:
- Detailed explanations and specifications
- Code examples and usage patterns
- API references and parameters
- Best practices and recommendations

## Examples

See [reference.md](reference.md) for comprehensive examples and use cases.
"#,
                overview,
                entry.title.to_lowercase()
            ));
        } else {
            let overview = if !entry.description.is_empty() {
                entry.description.clone()
            } else {
                format!("Information about {}.", entry.title.to_lowercase())
            };

            content.push_str(&format!(
                r#"

## Overview

{}

## How to Use

When you need information about {}:
1. Ask Claude about the specific aspect you need help with
2. Claude will provide guidance based on this skill's knowledge

## Reference

Original documentation: {}
"#,
                overview,
                entry.title.to_lowercase(),
                entry.url
            ));
        }

        content
    }

    /// Generate a Claude Skill from an llms.txt entry
    async fn generate_skill(&self, entry: &LlmsTxtEntry, source_url: &str) -> Result<PathBuf> {
        let skill_name = self.title_to_skill_name(&entry.title);
        let skill_dir = self.output_dir.join(&skill_name);

        std::fs::create_dir_all(&skill_dir)
            .context(format!("Failed to create skill directory: {:?}", skill_dir))?;

        // Fetch markdown content
        println!("  Fetching content from {}...", entry.url);
        let markdown_content = fetch_markdown_content(&self.client, &entry.url).await;

        // Generate SKILL.md
        let gerund_title = self.title_to_gerund(&entry.title);
        let description = self.create_description(entry);
        let has_reference = markdown_content.is_some();

        let skill_md = self.create_skill_md(&gerund_title, &description, entry, has_reference);

        let skill_md_path = skill_dir.join("SKILL.md");
        std::fs::write(&skill_md_path, skill_md)
            .context(format!("Failed to write SKILL.md: {:?}", skill_md_path))?;

        // Generate reference.md if we have content
        if let Some(content) = markdown_content {
            let reference_path = skill_dir.join("reference.md");
            if let Err(e) = std::fs::write(&reference_path, content) {
                println!("  Warning: Failed to write reference.md: {}", e);
            }
        }

        // Write metadata
        let metadata = SkillMetadata {
            source_url: source_url.to_string(),
            entry_url: entry.url.clone(),
            generated_at: Utc::now(),
            generator_version: VERSION.to_string(),
        };
        let metadata_path = skill_dir.join(".metadata.json");
        let metadata_json = serde_json::to_string_pretty(&metadata)
            .context("Failed to serialize metadata")?;
        std::fs::write(&metadata_path, metadata_json)
            .context(format!("Failed to write metadata: {:?}", metadata_path))?;

        Ok(skill_dir)
    }
}

/// Fetch llms.txt content from a URL
async fn fetch_llms_txt(client: &reqwest::Client, url: &str) -> Result<String> {
    let response = client
        .get(url)
        .send()
        .await
        .context(format!("Failed to fetch llms.txt from {}", url))?;

    let content = response
        .text()
        .await
        .context("Failed to read response text")?;

    Ok(content)
}

/// Parse llms.txt content and extract entries
fn parse_llms_txt(content: &str, base_url: &str) -> Result<Vec<LlmsTxtEntry>> {
    let re = Regex::new(r"^-\s+\[([^\]]+)\]\(([^\)]+)\)(?::\s+(.*))?$")
        .context("Failed to compile regex")?;

    let base = Url::parse(base_url).context("Invalid base URL")?;
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some(caps) = re.captures(line) {
            let title = caps.get(1).unwrap().as_str().to_string();
            let url_str = caps.get(2).unwrap().as_str();
            let description = caps.get(3).map(|m| m.as_str().to_string());

            // Resolve relative URLs
            let url = if url_str.starts_with("http://") || url_str.starts_with("https://") {
                url_str.to_string()
            } else {
                base.join(url_str)
                    .context(format!("Failed to resolve URL: {}", url_str))?
                    .to_string()
            };

            entries.push(LlmsTxtEntry::new(title, url, description));
        }
    }

    Ok(entries)
}

/// Fetch markdown content from a URL
async fn fetch_markdown_content(client: &reqwest::Client, url: &str) -> Option<String> {
    match client.get(url).send().await {
        Ok(response) => match response.text().await {
            Ok(text) => Some(text),
            Err(e) => {
                println!("  Warning: Failed to read response: {}", e);
                None
            }
        },
        Err(e) => {
            println!("  Warning: Failed to fetch {}: {}", url, e);
            None
        }
    }
}

/// Filter entries based on include/exclude patterns
fn apply_filters(
    entries: Vec<LlmsTxtEntry>,
    include_patterns: &[String],
    exclude_patterns: &[String],
) -> Result<Vec<LlmsTxtEntry>> {
    let include_set = build_globset(include_patterns)?;
    let exclude_set = build_globset(exclude_patterns)?;

    let filtered = entries
        .into_iter()
        .filter(|entry| {
            // Check exclude patterns first
            if !exclude_patterns.is_empty() && exclude_set.is_match(&entry.url) {
                return false;
            }

            // If include patterns specified, entry must match at least one
            if !include_patterns.is_empty() {
                return include_set.is_match(&entry.url);
            }

            true
        })
        .collect();

    Ok(filtered)
}

/// Build a GlobSet from pattern strings
fn build_globset(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).context(format!("Invalid glob pattern: {}", pattern))?;
        builder.add(glob);
    }
    builder.build().context("Failed to build GlobSet")
}

/// Capitalize the first character of a string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

/// Get the registry file path (current directory or custom path)
fn get_registry_path(custom_path: Option<PathBuf>) -> PathBuf {
    custom_path.unwrap_or_else(|| PathBuf::from(REGISTRY_FILENAME))
}

/// Load registry from TOML file
fn load_registry(path: &PathBuf) -> Result<RegistryConfig> {
    let content = std::fs::read_to_string(path)
        .context(format!("Failed to read registry file: {:?}", path))?;
    let config: RegistryConfig = toml::from_str(&content)
        .context("Failed to parse registry TOML")?;
    Ok(config)
}

/// Save registry to TOML file
fn save_registry(path: &PathBuf, config: &RegistryConfig) -> Result<()> {
    let content = toml::to_string_pretty(config)
        .context("Failed to serialize registry")?;
    std::fs::write(path, content)
        .context(format!("Failed to write registry file: {:?}", path))?;
    Ok(())
}

/// Read metadata from a skill directory
fn read_metadata(skill_dir: &PathBuf) -> Option<SkillMetadata> {
    let metadata_path = skill_dir.join(".metadata.json");
    let content = std::fs::read_to_string(&metadata_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Generate skills from a source (used by both standalone and update modes)
async fn generate_from_source(
    client: &reqwest::Client,
    source_url: &str,
    output_dir: &PathBuf,
    include: &[String],
    exclude: &[String],
) -> Result<(usize, usize)> {
    // Fetch llms.txt content
    println!("Fetching llms.txt from {}...", source_url);
    let llms_txt_content = fetch_llms_txt(client, source_url).await?;

    // Parse entries
    println!("Parsing entries...");
    let entries = parse_llms_txt(&llms_txt_content, source_url)?;
    println!("Found {} entries", entries.len());

    // Apply filters
    let entries = if !include.is_empty() || !exclude.is_empty() {
        let filtered = apply_filters(entries, include, exclude)?;
        println!("After filtering: {} entries", filtered.len());
        filtered
    } else {
        entries
    };

    if entries.is_empty() {
        println!("No entries to process after filtering");
        return Ok((0, 0));
    }

    // Generate skills
    println!("\nGenerating skills in {:?}...", output_dir);
    let generator = SkillGenerator::new(output_dir.clone(), client.clone())?;

    let mut success_count = 0;
    let mut failed_count = 0;

    for (i, entry) in entries.iter().enumerate() {
        println!("\n[{}/{}] Processing: {}", i + 1, entries.len(), entry.title);

        match generator.generate_skill(entry, source_url).await {
            Ok(path) => {
                println!("  ✓ Created skill: {}", path.display());
                success_count += 1;
            }
            Err(e) => {
                println!("  ✗ Failed: {}", e);
                failed_count += 1;
            }
        }
    }

    Ok((success_count, failed_count))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Create HTTP client for reuse across all requests
    let client = reqwest::Client::builder()
        .user_agent("claude-skill-gen/1.0 (https://github.com/anthropics/claude-code)")
        .timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .build()
        .context("Failed to build HTTP client")?;

    // Handle commands or standalone mode
    match &cli.command {
        Some(Commands::Init { registry }) => {
            let registry_path = get_registry_path(registry.clone());

            if registry_path.exists() {
                anyhow::bail!("Registry file already exists: {:?}", registry_path);
            }

            // Create empty registry with example entry
            let config = RegistryConfig {
                sources: vec![],
            };

            save_registry(&registry_path, &config)?;
            println!("Created registry file: {:?}", registry_path);
            println!("\nAdd sources with:");
            println!("  claude-skill-gen add <url>");
        }

        Some(Commands::Add { url, include, exclude, registry }) => {
            let registry_path = get_registry_path(registry.clone());

            let mut config = if registry_path.exists() {
                load_registry(&registry_path)?
            } else {
                RegistryConfig { sources: vec![] }
            };

            // Add new source
            config.sources.push(Source {
                url: url.clone(),
                include: include.clone(),
                exclude: exclude.clone(),
            });

            save_registry(&registry_path, &config)?;
            println!("Added source to registry: {}", url);
            if !include.is_empty() {
                println!("  Include: {:?}", include);
            }
            if !exclude.is_empty() {
                println!("  Exclude: {:?}", exclude);
            }
        }

        Some(Commands::List { registry }) => {
            let registry_path = get_registry_path(registry.clone());

            if !registry_path.exists() {
                anyhow::bail!("Registry file not found: {:?}\nRun 'claude-skill-gen init' to create one.", registry_path);
            }

            let config = load_registry(&registry_path)?;

            if config.sources.is_empty() {
                println!("No sources in registry");
                return Ok(());
            }

            println!("Registry sources ({}):", registry_path.display());
            for (i, source) in config.sources.iter().enumerate() {
                println!("\n{}. {}", i + 1, source.url);
                if !source.include.is_empty() {
                    println!("   Include: {:?}", source.include);
                }
                if !source.exclude.is_empty() {
                    println!("   Exclude: {:?}", source.exclude);
                }
            }
        }

        Some(Commands::Update { source, registry }) => {
            let registry_path = get_registry_path(registry.clone());

            if !registry_path.exists() {
                anyhow::bail!("Registry file not found: {:?}\nRun 'claude-skill-gen init' to create one.", registry_path);
            }

            let config = load_registry(&registry_path)?;

            if config.sources.is_empty() {
                anyhow::bail!("No sources in registry. Add sources with 'claude-skill-gen add <url>'");
            }

            // Filter sources if specific source requested
            let sources_to_update: Vec<&Source> = if let Some(source_filter) = source {
                config.sources.iter().filter(|s| &s.url == source_filter).collect()
            } else {
                config.sources.iter().collect()
            };

            if sources_to_update.is_empty() {
                anyhow::bail!("No matching sources found");
            }

            let mut total_success = 0;
            let mut total_failed = 0;

            for src in sources_to_update {
                println!("\n{}", "=".repeat(60));
                println!("Updating from: {}", src.url);
                println!("{}", "=".repeat(60));

                // Delete existing skills from this source
                if cli.output_dir.exists() {
                    for entry in std::fs::read_dir(&cli.output_dir)? {
                        let entry = entry?;
                        if entry.path().is_dir() {
                            if let Some(metadata) = read_metadata(&entry.path()) {
                                if metadata.source_url == src.url {
                                    println!("Removing old skill: {}", entry.path().display());
                                    std::fs::remove_dir_all(entry.path())?;
                                }
                            }
                        }
                    }
                }

                // Generate fresh skills
                let (success, failed) = generate_from_source(
                    &client,
                    &src.url,
                    &cli.output_dir,
                    &src.include,
                    &src.exclude,
                ).await?;

                total_success += success;
                total_failed += failed;
            }

            // Summary
            println!("\n{}", "=".repeat(60));
            println!("Update Summary:");
            println!("  Successfully generated: {} skills", total_success);
            println!("  Failed: {} skills", total_failed);
            println!("  Output directory: {}", cli.output_dir.display());
            println!("{}", "=".repeat(60));

            if total_success > 0 {
                println!("\nTo install skills:");
                println!("  Personal: cp -r {}/* ~/.claude/skills/", cli.output_dir.display());
                println!("  Project:  cp -r {}/* ./.claude/skills/", cli.output_dir.display());
            }
        }

        None => {
            // Standalone mode - use provided URL
            if let Some(url) = &cli.url {
                let (success_count, failed_count) = generate_from_source(
                    &client,
                    url,
                    &cli.output_dir,
                    &cli.include,
                    &cli.exclude,
                ).await?;

                // Summary
                println!("\n{}", "=".repeat(60));
                println!("Summary:");
                println!("  Successfully generated: {} skills", success_count);
                println!("  Failed: {} skills", failed_count);
                println!("  Output directory: {}", cli.output_dir.display());
                println!("{}", "=".repeat(60));

                if success_count > 0 {
                    println!("\nTo install skills:");
                    println!("  Personal: cp -r {}/* ~/.claude/skills/", cli.output_dir.display());
                    println!("  Project:  cp -r {}/* ./.claude/skills/", cli.output_dir.display());
                }
            } else {
                anyhow::bail!("No URL provided. Use 'claude-skill-gen <url>' or see 'claude-skill-gen --help'");
            }
        }
    }

    Ok(())
}
