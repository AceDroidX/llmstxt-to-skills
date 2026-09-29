# llmstxt-to-skills

Parses llms.txt files and generates Claude Skills with proper YAML frontmatter and reference documentation.

## What this does

Takes a URL to an llms.txt file, fetches all the linked markdown documentation, and generates ONE Claude Skill per source domain. The skill includes a comprehensive SKILL.md with table of contents and a references/ directory containing all the documentation as individual markdown files. This matches the official Anthropic Agent Skills pattern.

## Building

Requires Rust 1.70 or later.

```bash
cargo build --release
```

The binary will be at `target/release/claude-skill-gen`.

## Usage

Basic usage:

```bash
./target/release/claude-skill-gen https://docs.anthropic.com/llms.txt
```

This creates skills in `./skills/` by default.

Set the generated folder name and the `name` in `SKILL.md`:

```bash
./target/release/claude-skill-gen https://docs.anthropic.com/llms.txt --name anthropic-docs
```

Names use lowercase letters, digits, and hyphens. Without `--name`, the source domain determines the name.

Custom output directory:

```bash
./target/release/claude-skill-gen https://docs.anthropic.com/llms.txt --output-dir ~/my-skills
```

Filter which entries to process:

```bash
# Only generate skills from API documentation
./target/release/claude-skill-gen https://docs.anthropic.com/llms.txt --include '*/api/*'

# Skip admin API documentation
./target/release/claude-skill-gen https://docs.anthropic.com/llms.txt --exclude '*/admin-api/*'

# Combine filters
./target/release/claude-skill-gen https://docs.anthropic.com/llms.txt \
  --include '*/api/*' \
  --exclude '*/admin-api/*'
```

## Registry Mode

For managing multiple documentation sources and keeping skills up-to-date, use the registry feature.

### Initialize a registry

```bash
./target/release/claude-skill-gen init
```

Creates `.claude-skills-registry.toml` in the current directory with an empty sources list.

### Add sources to registry

```bash
# Add a source without filters
./target/release/claude-skill-gen add https://docs.anthropic.com/llms.txt

# Add a source with filters
./target/release/claude-skill-gen add https://docs.anthropic.com/llms.txt \
  --include '*/api/*' \
  --exclude '*/admin-api/*'

# Add another source
./target/release/claude-skill-gen add https://other-docs.com/llms.txt

# Save a custom name for future updates
./target/release/claude-skill-gen add https://docs.example.com/llms.txt --name example-docs
```

### List registered sources

```bash
./target/release/claude-skill-gen list
```

Shows all sources in the registry with their filters.

### Update skills from registry

```bash
# Update all sources
./target/release/claude-skill-gen update

# Update only one source
./target/release/claude-skill-gen update --source https://docs.anthropic.com/llms.txt

# Override the saved name while updating one source
./target/release/claude-skill-gen update --source https://docs.example.com/llms.txt --name new-example-docs
```

An update uses the name saved with the source unless `--name` overrides it. An override applies to only one source.

The update command:
1. Scans the output directory for skill directories
2. Checks `.metadata.json` in each to find matching source URLs
3. Deletes the entire skill directory for sources being updated
4. Fetches fresh documentation from the llms.txt source
5. Generates a new domain skill with all current references

This ensures you always have current documentation without manually tracking what came from where. Since each source generates exactly one skill directory, updates are straightforward.

### Registry file format

`.claude-skills-registry.toml`:

```toml
[[source]]
url = "https://docs.anthropic.com/llms.txt"
include = ["*/api/*"]
exclude = ["*/admin-api/*"]

[[source]]
url = "https://other-docs.com/llms.txt"
name = "other-docs"
```

### Metadata tracking

Each generated skill includes `.metadata.json`:

```json
{
  "source_url": "https://docs.claude.com/llms.txt",
  "domain": "docs-claude-com",
  "entry_count": 127,
  "sections": ["Getting Started", "API Reference", "Guides", "Examples"],
  "generated_at": "2025-01-18T10:30:00Z",
  "generator_version": "1.0.0"
}
```

This allows `update` to identify which skills to regenerate. Since each source generates exactly one skill, the update process simply deletes the matching domain directory and regenerates it.

## Installing generated skills

Copy the generated skill directories to your Claude skills directory:

```bash
# For personal use (all projects)
cp -r ./skills/* ~/.claude/skills/

# For project use (share with team)
cp -r ./skills/* ./.claude/skills/
git add .claude/skills/
git commit -m "Add documentation skills"
```

Each source generates one skill directory (e.g., `docs-claude-com/`), so you can easily manage and update individual documentation sources.

## How it works

1. Fetches the llms.txt file from the provided URL
2. Parses the complete structure: H1 title, blockquote summary, and H2 sections with entries
3. Extracts domain name from URL (e.g., `docs.claude.com` → `docs-claude-com`)
4. Downloads markdown content from each linked entry
5. Generates a single skill directory per source:
   - `SKILL.md` - YAML frontmatter, overview, and organized table of contents linking to all references
   - `references/` - Directory containing one markdown file per entry
   - `.metadata.json` - Tracking info (domain, source URL, entry count, sections) for registry updates

### Example generated structure

```
skills/
└── docs-claude-com/
    ├── SKILL.md                    # Main skill with TOC
    ├── .metadata.json               # Metadata for updates
    └── references/
        ├── getting_api_keys.md      # Each entry becomes a reference
        ├── creating_messages.md
        ├── streaming_responses.md
        └── ...
```

The SKILL.md organizes all references by their original H2 sections from llms.txt, making it easy for Claude to navigate the documentation.

## llms.txt format

Expects markdown lists with links:

```markdown
# Section Name

- [Entry Title](https://example.com/doc.md)
- [Another Entry](https://example.com/other.md): Optional description
- [Third Entry](./relative/path.md): Descriptions are used in skill metadata
```

Relative URLs are resolved based on the llms.txt file location.

## Performance

Uses a single HTTP client with connection pooling. Should process large documentation sets reasonably fast. If requests seem slow, the target server may be rate limiting.

## Requirements

- Rust toolchain (for building)
- Internet connection (for fetching documentation)

## Troubleshooting

**Compilation fails:**
```bash
rustup update
cargo clean
cargo build --release
```

**Can't find cargo:**
Install Rust from https://rustup.rs

**Network errors:**
Check the URL is accessible. Some documentation sites may require authentication or have rate limiting.

**Skills not loading in Claude Code:**
- Verify installation location (`~/.claude/skills/` or `./.claude/skills/`)
- Check YAML frontmatter in generated SKILL.md files
- Restart Claude Code

## License

MIT
