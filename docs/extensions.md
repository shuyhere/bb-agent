# Extensions & Skills

BB-Agent supports two extension mechanisms: **skills** (markdown instructions) and **extensions** (JS/TS plugins).

## Skills

Skills are markdown files that provide contextual instructions to the agent. They're listed in the system prompt and the agent reads them when relevant.

### Creating a Skill

Create a directory with a `SKILL.md` file:

```
~/.bb-agent/skills/my-skill/SKILL.md
```

Use YAML frontmatter for metadata:

```markdown
---
name: my-skill
description: Helps with deploying to production
---

## Deployment Instructions

When the user asks about deployment:

1. Check the environment with `bash: env | grep DEPLOY`
2. Run the deploy script: `bash: ./deploy.sh`
3. Verify with: `bash: curl -s https://api.example.com/health`
```

### Skill Discovery Paths

Skills are auto-discovered from:

| Location | Scope |
|----------|-------|
| `~/.bb-agent/skills/` | Global |
| `<project>/.bb-agent/skills/` | Project-local |
| `~/.agents/skills/` | Shared (pi-compatible) |
| `<ancestors>/.agents/skills/` | Ancestor directories |
| `settings.json` → `"skills": [...]` | Explicit paths |

### Skill Packages

Install skills from npm or git:

```bash
bb install npm:some-skill-package     # Global install
bb install --local npm:my-skill       # Project-local install
bb install git:https://github.com/org/skills.git
```

A skill package is an npm package with a `bb` field in `package.json`:

```json
{
  "name": "my-skill-package",
  "bb": {
    "skills": ["skills/"],
    "extensions": ["extensions/"],
    "prompts": ["prompts/"]
  }
}
```

Or simply place resources in `skills/`, `extensions/`, `prompts/` directories.

## Extensions (JS/TS Plugins)

Extensions are JavaScript or TypeScript files that can register custom tools, commands, and event hooks.

### Loading Extensions

| Location | Scope |
|----------|-------|
| `~/.bb-agent/extensions/` | Global |
| `<project>/.bb-agent/extensions/` | Project-local |
| `settings.json` → `"extensions": [...]` | Explicit paths |
| CLI: `bb -e ./my-extension.ts` | Ad-hoc |

### Extension API

Extensions communicate with BB-Agent via stdin/stdout JSON protocol. They can:

- **Register tools** — custom tools the agent can call
- **Register commands** — slash commands (e.g., `/mycommand`)
- **Hook events** — intercept session start, input, tool calls, etc.

### Example Extension Structure

```
my-extension/
├── package.json
├── index.ts          # Entry point
└── tsconfig.json
```

## Prompt Templates

Reusable prompts invoked with `/name` in the input.

### Creating Prompts

Place `.md` files in the prompts directory:

```
~/.bb-agent/prompts/review.md
```

```markdown
Review the code in the current directory for:
- Security vulnerabilities
- Performance issues
- Code style problems
Provide a structured report.
```

Then use in BB-Agent:
```
/review
```

### Prompt Discovery Paths

| Location | Scope |
|----------|-------|
| `~/.bb-agent/prompts/` | Global |
| `<project>/.bb-agent/prompts/` | Project-local |
| `settings.json` → `"prompts": [...]` | Explicit paths |

## Package Management

```bash
bb install <source>           # Install globally
bb install --local <source>   # Install into project
bb remove <source>            # Remove a package
bb list                       # List all packages
bb list --local               # List project packages
bb list --global              # List global packages
bb update                     # Update all packages
```

### Package Sources

| Format | Example |
|--------|---------|
| npm | `npm:package-name` |
| git | `git:https://github.com/org/repo.git` |
| local path | `./my-local-skill` or `/absolute/path` |
| URL | `https://example.com/package.tar.gz` |

### Package Filtering

In `settings.json`, you can filter which resources a package provides:

```json
{
  "packages": [
    {
      "source": "npm:big-package",
      "skills": ["skills/only-this-one/**"],
      "extensions": [],
      "prompts": ["*"]
    }
  ]
}
```
