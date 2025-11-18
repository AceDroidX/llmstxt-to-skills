# llmstxt-to-skills

Parses llms.txt files and generates Claude Skills with proper YAML frontmatter and reference documentation.

## What this does

Takes a URL to an llms.txt file, fetches all the linked markdown documentation, and generates Claude Skills that can be loaded into Claude Code. Each entry becomes a separate skill with the fetched content as reference material.

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

## Installing generated skills

Copy the generated skills to your Claude skills directory:

```bash
# For personal use (all projects)
cp -r ./skills/* ~/.claude/skills/

# For project use (share with team)
cp -r ./skills/* ./.claude/skills/
git add .claude/skills/
git commit -m "Add skills"
```

## How it works

1. Fetches the llms.txt file from the provided URL
2. Parses entries using regex to extract titles, URLs, and descriptions
3. Downloads markdown content from each linked URL
4. Generates skill directories with:
   - `SKILL.md` - Contains YAML frontmatter and skill structure
   - `reference.md` - The fetched markdown documentation

Skill names are automatically converted to gerund form where possible (e.g., "Get API Key" becomes "Getting API Key" with directory name `getting-api-key`).

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
