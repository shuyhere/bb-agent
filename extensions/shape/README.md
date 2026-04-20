# bb-shape: Anything-to-Agent

Turn any URL, document, or description into a fully configured specialized agent.

## Install

```bash
bb install ./extensions/shape
```

## Usage

There is a single slash command, `/shape`. Everything else is an argument.

| Command | Flow |
|---|---|
| `/shape` | Open the menu |
| `/shape new` | Create a new agent (main flow) |
| `/shape list` | Browse & activate existing agents |
| `/shape organize` | Rename, remove, or inspect agents |
| `/shape load <name-or-id>` | Quick-activate by exact or partial name/id |
| `/shape 1` .. `/shape 4` | Numeric shortcuts for the menu items |
| `/shape help [sub]` | Show detailed help for a sub-command |

Typing `/shape` with no args opens an interactive picker below the input.
Use ↑↓ and Enter. Picking an item starts that flow immediately.

If you want the long explainer for a sub-command, use:

```bash
/shape help new
/shape help list
/shape help organize
/shape help load
```

### Creating an agent (`/shape new`)

1. **Resources** — enter URLs, files, or descriptions (comma-separated).
2. **Identity** — describe the agent's role in 1–2 sentences.
3. The agent ingests sources, confirms the plan, builds the full agent.
4. Done — saved to `~/.bb-agent/agents/`.

### Loading an existing agent (`/shape load <name-or-id>`)

- exact id matches activate immediately
- exact or partial name matches are resolved from `~/.bb-agent/agents/registry.json`
- if multiple agents match, Shape opens a picker so you can choose the right one
- if no query is provided, Shape asks for one with a local prompt

### Managing agents (`/shape organize`)

- **Rename** → pick agent → enter new name
- **Remove** → pick agent → confirm deletion
- **View details** → prints full `agent.json`

## Architecture

```text
extensions/shape/
├── package.json                 # Registers extension + skills
├── src/
│   └── index.js                 # Extension: registers /shape via bb.registerCommand
├── skills/shape/
│   ├── SKILL.md                 # Agent instructions for building agents
│   └── references/
│       ├── agent_templates.md   # Role templates
│       ├── crawl_strategy.md    # Deep crawl instructions
│       └── skill_templates.md   # Skill patterns (includes search_knowledge.py)
├── tools/
│   └── search_knowledge.py      # Bundled search tool copied into generated agents
└── README.md
```

The extension (`src/index.js`) stays intentionally small. It:

- registers the `/shape` command
- returns structured menu/prompt/dispatch outcomes that BB-Agent's TUI knows how to render
- reads the local shaped-agent registry for list/load flows

The heavy lifting — crawling, ingesting, writing manifests, and updating the registry — lives in the skill.

This keeps the extension dependency-free in the npm sense and avoids network work in the JS layer, while still allowing the extension to read local state for agent activation flows.

## Notes on the interactive menu

`/shape` opens a native select menu in the BB-Agent TUI — the same style of UX as `/settings`.
Picking an item re-invokes `/shape <value>`, which starts the selected flow immediately rather than dumping a long help block into the transcript.

Detailed documentation is still available explicitly via `/shape help <sub>`.

The extension uses structured command results such as:

```js
// /shape
return {
  menu: {
    title: '🔷 Shape — Anything-to-Agent',
    items: [
      { label: '✨ New', detail: 'Create a new agent…', value: 'new' },
      { label: '📋 List', detail: 'Browse & activate…', value: 'list' },
      { label: '🗂  Organize', detail: 'Rename, remove, or inspect…', value: 'organize' },
      { label: '🔄 Load', detail: 'Quick-activate by name or ID', value: 'load' },
    ],
  },
};

// /shape new
return {
  prompt: {
    title: '🔷 Shape — New Agent',
    lines: ['Give me your resources.'],
    inputLabel: 'Resources',
    resume: 'opaque-state-token',
  },
};
```

## Storage

```text
~/.bb-agent/agents/
├── registry.json                    # Index of all agents
├── mystore-support-a1b2c3/          # One directory per agent
└── ...
```

Registry schema:

```json
{
  "version": 1,
  "agents": [
    {
      "id": "mystore-support-a1b2c3",
      "name": "MyStore Support",
      "role": "Customer Support Agent",
      "source_summary": "mystore.com",
      "created_at": "2025-04-07T13:42:00Z",
      "path": "mystore-support-a1b2c3"
    }
  ]
}
```
