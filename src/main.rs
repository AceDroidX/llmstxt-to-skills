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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

/// Metadata stored with each generated skill
#[derive(Debug, Serialize, Deserialize)]
struct SkillMetadata {
    source_url: String,
    domain: String,
    entry_count: usize,
    sections: Vec<String>,
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
    #[arg(global = true)]
    url: Option<String>,

    /// Output directory for generated skills
    #[arg(short, long, default_value = "./skills", global = true)]
    output_dir: PathBuf,

    /// Skill folder and SKILL.md name (defaults to the source domain)
    #[arg(long, global = true, value_parser = validate_skill_name)]
    name: Option<String>,

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
    section: String,
}

impl LlmsTxtEntry {
    fn new(title: String, url: String, description: Option<String>, section: String) -> Self {
        Self {
            title,
            url,
            description: description.unwrap_or_default(),
            section,
        }
    }
}

/// Represents a complete parsed llms.txt file with title, summary, and organized sections
#[derive(Debug, Clone)]
struct ParsedLlmsTxt {
    title: Option<String>,
    summary: Option<String>,
    sections: Vec<(String, Vec<LlmsTxtEntry>)>,
}

/// Generates Claude Skills from llms.txt entries
struct SkillGenerator {
    output_dir: PathBuf,
    client: reqwest::Client,
}

impl SkillGenerator {
    fn new(output_dir: PathBuf, client: reqwest::Client) -> Result<Self> {
        std::fs::create_dir_all(&output_dir)
            .context(format!("Failed to create output directory: {:?}", output_dir))?;

        Ok(Self {
            output_dir,
            client,
        })
    }

    /// Generate a domain-level Claude Skill from parsed llms.txt content
    async fn generate_domain_skill(
        &self,
        parsed: &ParsedLlmsTxt,
        source_url: &str,
        domain: &str,
        skill_name: &str,
    ) -> Result<PathBuf> {
        let skill_dir = self.output_dir.join(skill_name);
        ensure_destination_available(&skill_dir, source_url)?;
        let references_dir = skill_dir.join("references");

        // Create skill and references directories
        std::fs::create_dir_all(&references_dir)
            .context(format!("Failed to create references directory: {:?}", references_dir))?;

        // Count total entries across all sections
        let total_entries: usize = parsed.sections.iter().map(|(_, entries)| entries.len()).sum();

        // Generate description (200-1024 chars) from title, summary, and sections
        let skill_title = parsed
            .title
            .as_ref()
            .unwrap_or(&domain.to_string())
            .clone();

        let mut description = if let Some(summary) = &parsed.summary {
            format!("{}.", summary.trim_end_matches('.'))
        } else {
            format!("Documentation and reference materials for {}.", skill_title)
        };

        // Add section overview
        if !parsed.sections.is_empty() {
            let section_names: Vec<String> = parsed
                .sections
                .iter()
                .map(|(name, _)| name.clone())
                .collect();
            description.push_str(&format!(
                " Contains {} reference documents organized into sections: {}.",
                total_entries,
                section_names.join(", ")
            ));
        }

        // Ensure description is at least 200 chars
        while description.len() < 200 {
            description.push_str(&format!(
                " Use this skill when working with {} documentation or when the user mentions topics covered in these references.",
                skill_title
            ));
        }

        // Truncate if too long
        if description.len() > 1024 {
            description.truncate(1021);
            description.push_str("...");
        }

        // Generate comprehensive SKILL.md
        let mut skill_md = format!(
            r#"---
name: {}
description: {}
version: 1.0.0
---

# {}

"#,
            skill_name, description, skill_title
        );

        // Add overview from summary
        if let Some(summary) = &parsed.summary {
            skill_md.push_str(&format!(
                r#"## Overview

{}

"#,
                summary
            ));
        }

        // Add usage instructions
        skill_md.push_str(&format!(
            r#"## How to Use This Skill

This skill contains {} reference documents organized by topic. When you need information about {}:

1. Claude will automatically access relevant reference files based on your question
2. Reference files are organized in the `references/` directory by topic
3. Each reference contains detailed documentation extracted from the official source

"#,
            total_entries, skill_title
        ));

        // Generate table of contents organized by section
        skill_md.push_str("## Reference Documentation\n\n");

        for (section_name, entries) in &parsed.sections {
            skill_md.push_str(&format!("### {}\n\n", section_name));

            for entry in entries {
                let filename = entry_title_to_filename(&entry.title);
                let description_text = if !entry.description.is_empty() {
                    format!(" - {}", entry.description)
                } else {
                    String::new()
                };
                skill_md.push_str(&format!(
                    "- [{}](references/{}){}\n",
                    entry.title, filename, description_text
                ));
            }

            skill_md.push_str("\n");
        }

        // Write SKILL.md
        let skill_md_path = skill_dir.join("SKILL.md");
        std::fs::write(&skill_md_path, skill_md)
            .context(format!("Failed to write SKILL.md: {:?}", skill_md_path))?;

        println!("  Created SKILL.md");

        // Generate reference files for each entry
        let mut entry_count = 0;
        for (section_name, entries) in &parsed.sections {
            println!("\n  Processing section: {}", section_name);

            for entry in entries {
                let filename = entry_title_to_filename(&entry.title);
                let reference_path = references_dir.join(&filename);

                println!("    Fetching: {} -> {}", entry.title, filename);

                // Fetch content
                match fetch_markdown_content(&self.client, &entry.url).await {
                    Some(content) => {
                        // Create reference file with frontmatter
                        let mut reference_content = format!(
                            r#"# {}

**Source:** {}
**Section:** {}

"#,
                            entry.title, entry.url, section_name
                        );

                        if !entry.description.is_empty() {
                            reference_content.push_str(&format!(
                                "**Description:** {}\n\n---\n\n",
                                entry.description
                            ));
                        } else {
                            reference_content.push_str("---\n\n");
                        }

                        reference_content.push_str(&content);

                        std::fs::write(&reference_path, reference_content).context(format!(
                            "Failed to write reference file: {:?}",
                            reference_path
                        ))?;

                        entry_count += 1;
                        println!("      ✓ Saved");
                    }
                    None => {
                        println!("      ✗ Failed to fetch content");
                    }
                }
            }
        }

        // Collect all section names
        let section_names: Vec<String> = parsed
            .sections
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        // Write metadata
        let metadata = SkillMetadata {
            source_url: source_url.to_string(),
            domain: domain.to_string(),
            entry_count,
            sections: section_names,
            generated_at: Utc::now(),
            generator_version: VERSION.to_string(),
        };
        let metadata_path = skill_dir.join(".metadata.json");
        let metadata_json =
            serde_json::to_string_pretty(&metadata).context("Failed to serialize metadata")?;
        std::fs::write(&metadata_path, metadata_json)
            .context(format!("Failed to write metadata: {:?}", metadata_path))?;

        println!("\n  ✓ Skill complete: {} references in {} sections", entry_count, parsed.sections.len());

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

/// Parse llms.txt content and extract title, summary, and organized sections
fn parse_llms_txt(content: &str, base_url: &str) -> Result<ParsedLlmsTxt> {
    let h1_re = Regex::new(r"^# (.+)$").context("Failed to compile H1 regex")?;
    let h2_re = Regex::new(r"^## (.+)$").context("Failed to compile H2 regex")?;
    let entry_re = Regex::new(r"^-\s+\[([^\]]+)\]\(([^\)]+)\)(?::\s+(.*))?$")
        .context("Failed to compile entry regex")?;

    let base = Url::parse(base_url).context("Invalid base URL")?;

    let mut title: Option<String> = None;
    let mut summary_lines: Vec<String> = Vec::new();
    let mut sections: Vec<(String, Vec<LlmsTxtEntry>)> = Vec::new();
    let mut current_section: Option<String> = None;
    let mut current_entries: Vec<LlmsTxtEntry> = Vec::new();
    let mut in_blockquote = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Extract H1 title
        if let Some(caps) = h1_re.captures(trimmed) {
            if title.is_none() {
                title = Some(caps.get(1).unwrap().as_str().to_string());
            }
            continue;
        }

        // Extract blockquote summary (consecutive lines starting with >)
        if trimmed.starts_with('>') {
            in_blockquote = true;
            let summary_line = trimmed.trim_start_matches('>').trim().to_string();
            if !summary_line.is_empty() {
                summary_lines.push(summary_line);
            }
            continue;
        } else if in_blockquote && !trimmed.is_empty() && !trimmed.starts_with('#') {
            // Continue blockquote if next line is not empty and not a header
            continue;
        } else {
            in_blockquote = false;
        }

        // Track H2 sections
        if let Some(caps) = h2_re.captures(trimmed) {
            // Save previous section if it exists
            if let Some(section_name) = current_section.take() {
                if !current_entries.is_empty() {
                    sections.push((section_name, current_entries.clone()));
                    current_entries.clear();
                }
            }
            // Start new section
            current_section = Some(caps.get(1).unwrap().as_str().to_string());
            continue;
        }

        // Parse entries
        if let Some(caps) = entry_re.captures(trimmed) {
            let entry_title = caps.get(1).unwrap().as_str().to_string();
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

            let section_name = current_section.clone().unwrap_or_else(|| "General".to_string());
            current_entries.push(LlmsTxtEntry::new(entry_title, url, description, section_name));
        }
    }

    // Save final section
    if let Some(section_name) = current_section {
        if !current_entries.is_empty() {
            sections.push((section_name, current_entries));
        }
    } else if !current_entries.is_empty() {
        // Handle entries without sections
        sections.push(("General".to_string(), current_entries));
    }

    let summary = if summary_lines.is_empty() {
        None
    } else {
        Some(summary_lines.join(" "))
    };

    Ok(ParsedLlmsTxt {
        title,
        summary,
        sections,
    })
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

/// Extract domain name from source URL and sanitize for filesystem use
fn extract_domain_name(source_url: &str) -> Result<String> {
    let url = Url::parse(source_url)
        .context(format!("Failed to parse source URL: {}", source_url))?;

    let domain = url.host_str()
        .context("No host in URL")?;

    // Sanitize for filesystem: replace dots with hyphens, lowercase, remove invalid chars
    let sanitized = domain
        .to_lowercase()
        .replace('.', "-");

    // Remove any remaining invalid filesystem characters
    let re = Regex::new(r"[^\w\-]").unwrap();
    let clean = re.replace_all(&sanitized, "");

    Ok(clean.to_string())
}

/// Keep custom names safe as a single directory and a plain YAML value.
fn validate_skill_name(name: &str) -> std::result::Result<String, String> {
    let valid = !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(name.to_string())
    } else {
        Err("name must use lowercase letters, digits, and single hyphens between them".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_name_is_parsed_and_validated() {
        let cli = Cli::try_parse_from([
            "claude-skill-gen",
            "https://example.com/llms.txt",
            "--name",
            "my-docs",
        ])
        .unwrap();
        assert_eq!(cli.name.as_deref(), Some("my-docs"));

        for invalid in ["../other", "My Docs", "my_docs", "-docs", "docs-"] {
            assert!(validate_skill_name(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn registry_name_is_optional_and_persisted() {
        let legacy: RegistryConfig = toml::from_str("[[source]]\nurl = 'https://example.com/llms.txt'\n")
            .unwrap();
        assert!(legacy.sources[0].name.is_none());

        let source = Source {
            url: "https://example.com/llms.txt".to_string(),
            name: Some("my-docs".to_string()),
            include: vec![],
            exclude: vec![],
        };
        let content = toml::to_string(&RegistryConfig {
            sources: vec![source],
        })
        .unwrap();
        let saved: RegistryConfig = toml::from_str(&content).unwrap();
        assert_eq!(saved.sources[0].name.as_deref(), Some("my-docs"));
    }

    #[tokio::test]
    async fn custom_name_sets_folder_and_frontmatter() {
        let output_dir = std::env::temp_dir().join(format!(
            "claude-skill-gen-test-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let client = reqwest::Client::new();
        let generator = SkillGenerator::new(output_dir.clone(), client).unwrap();
        let parsed = ParsedLlmsTxt {
            title: Some("Example Docs".to_string()),
            summary: None,
            sections: vec![],
        };

        let skill_dir = generator
            .generate_domain_skill(
                &parsed,
                "https://example.com/llms.txt",
                "example-com",
                "my-docs",
            )
            .await
            .unwrap();
        assert_eq!(skill_dir, output_dir.join("my-docs"));
        let skill_md = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(skill_md.starts_with("---\nname: my-docs\n"));
        let metadata = read_metadata(&skill_dir).unwrap();
        assert_eq!(metadata.domain, "example-com");
        std::fs::remove_dir_all(output_dir).unwrap();
    }
}

/// Convert entry title to filesystem-safe filename for reference files
fn entry_title_to_filename(title: &str) -> String {
    // Convert to lowercase and replace spaces/hyphens with underscores
    let lowercase = title.to_lowercase();
    let with_underscores = lowercase.replace(' ', "_").replace('-', "_");

    // Remove special characters, keep only word chars and underscores
    let re = Regex::new(r"[^\w_]").unwrap();
    let clean = re.replace_all(&with_underscores, "");

    // Trim leading/trailing underscores and add .md extension
    let trimmed = clean.trim_matches('_');

    format!("{}.md", trimmed)
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
    for source in &config.sources {
        if let Some(name) = &source.name {
            validate_skill_name(name)
                .map_err(|error| anyhow::anyhow!("Invalid name for {}: {}", source.url, error))?;
        }
    }
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

fn ensure_destination_available(skill_dir: &PathBuf, source_url: &str) -> Result<()> {
    if skill_dir.exists()
        && read_metadata(skill_dir)
            .map(|metadata| metadata.source_url != source_url)
            .unwrap_or(true)
    {
        anyhow::bail!(
            "Skill directory {} already exists for another source",
            skill_dir.display()
        );
    }
    Ok(())
}

/// Generate domain skill from a source (used by both standalone and update modes)
async fn generate_from_source(
    client: &reqwest::Client,
    source_url: &str,
    output_dir: &PathBuf,
    name: Option<&str>,
    include: &[String],
    exclude: &[String],
) -> Result<(usize, usize)> {
    // Fetch llms.txt content
    println!("Fetching llms.txt from {}...", source_url);
    let llms_txt_content = fetch_llms_txt(client, source_url).await?;

    // Parse llms.txt
    println!("Parsing llms.txt...");
    let mut parsed = parse_llms_txt(&llms_txt_content, source_url)?;

    // Count total entries
    let total_entries: usize = parsed.sections.iter().map(|(_, entries)| entries.len()).sum();
    println!(
        "Found {} entries in {} sections",
        total_entries,
        parsed.sections.len()
    );

    // Apply filters if specified
    if !include.is_empty() || !exclude.is_empty() {
        println!("Applying filters...");

        // Flatten, filter, and reconstruct sections
        let mut filtered_sections: Vec<(String, Vec<LlmsTxtEntry>)> = Vec::new();

        for (section_name, entries) in parsed.sections {
            let filtered_entries = apply_filters(entries, include, exclude)?;

            if !filtered_entries.is_empty() {
                filtered_sections.push((section_name, filtered_entries));
            }
        }

        parsed.sections = filtered_sections;

        let filtered_count: usize = parsed.sections.iter().map(|(_, entries)| entries.len()).sum();
        println!("After filtering: {} entries in {} sections", filtered_count, parsed.sections.len());

        if filtered_count == 0 {
            println!("No entries remaining after filtering");
            return Ok((0, 1));
        }
    }

    // Extract domain name
    let domain = extract_domain_name(source_url)?;
    let skill_name = name.unwrap_or(&domain);
    println!("\nGenerating skill: {}", skill_name);

    // Generate domain skill
    let generator = SkillGenerator::new(output_dir.clone(), client.clone())?;

    match generator.generate_domain_skill(&parsed, source_url, &domain, skill_name).await {
        Ok(skill_dir) => {
            println!("\n✓ Successfully generated domain skill: {}", skill_dir.display());
            Ok((1, 0))
        }
        Err(e) => {
            println!("\n✗ Failed to generate skill: {}", e);
            Ok((0, 1))
        }
    }
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
                name: cli.name.clone(),
                include: include.clone(),
                exclude: exclude.clone(),
            });

            save_registry(&registry_path, &config)?;
            println!("Added source to registry: {}", url);
            if let Some(name) = &cli.name {
                println!("  Name: {}", name);
            }
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
                if let Some(name) = &source.name {
                    println!("   Name: {}", name);
                }
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

            if cli.name.is_some() && sources_to_update.len() != 1 {
                anyhow::bail!("--name requires updating exactly one source; use --source to select it");
            }

            let mut total_success = 0;
            let mut total_failed = 0;

            for src in sources_to_update {
                println!("\n{}", "=".repeat(60));
                println!("Updating from: {}", src.url);
                println!("{}", "=".repeat(60));

                let domain = extract_domain_name(&src.url)?;
                let skill_name = cli.name.as_deref().or(src.name.as_deref()).unwrap_or(&domain);
                let target_dir = cli.output_dir.join(skill_name);
                ensure_destination_available(&target_dir, &src.url)?;

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
                    cli.name.as_deref().or(src.name.as_deref()),
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
                    cli.name.as_deref(),
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
