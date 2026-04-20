---
name: shape
description: "Build, list, load, and manage specialized agents from URLs, documents, or text. Use whenever the user runs /shape (with optional args: new, list, organize, load <name>) or asks to 'make/build an agent for ...'."
---

# Shape: Anything-to-Agent

Turn any source into a specialized agent. Manage all your shaped agents.

There is exactly **one slash command**: `/shape`. Everything else is an argument.

| Invocation | What it does |
|---|---|
| `/shape` | Show the menu — list the options below and ask the user to pick |
| `/shape new` | Create a new agent — run the `## New` flow |
| `/shape list` | Browse & activate existing agents — run the `## List` flow |
| `/shape organize` | Rename, remove, or inspect agents — run the `## Organize` flow |
| `/shape load <name>` | Quick-activate by name or id — run the `## Load` flow |

The bb-shape extension (`src/index.js`) registers `/shape` and parses these
sub-args. When the extension is loaded, invoking `/shape` without arguments
returns a structured native menu in the TUI. The extension also uses structured
prompt/dispatch outcomes for the guided wizard steps.

When the extension is not loaded, this skill can still be used directly when
the user asks to make or manage an agent, but there is no separate
`prompts/shape.md` fallback file in this package.

Numeric shortcuts are accepted: `/shape 1` → `new`, `/shape 2` → `list`, etc.

---

## Menu

When `/shape` is invoked without arguments, present:

```
🔷 Shape — Anything-to-Agent

  1. ✨ New        Create a new agent from URLs, docs, or descriptions
  2. 📋 List       Browse & activate existing agents
  3. 🗂  Organize  Rename, remove, or inspect agents
  4. 🔄 Load       Quick-activate an agent by name or ID

Pick a number, or type /shape new | list | organize | load <name>.
```

When the user picks a number, execute the corresponding flow below.

---

## New

Interactive agent creation. Walk the user through these steps in order.

### Step 1: Collect Resources

Ask the user to provide their source materials. Multiple resources separated by commas.

```
📦 Let's build a new agent. Give me your resources:

  Separate multiple sources with commas.

  Supported types:
    🌐 URLs         — https://docs.example.com, https://shop.example.com/faq
    📄 Documents     — ./handbook.pdf, ~/notes.md, /path/to/spec.txt
    💬 Description   — "A pizza restaurant in Brooklyn called Sal's, open 11am-11pm"

  Example:
    https://mystore.com, https://mystore.com/faq, "We do free returns within 30 days"

> Resources:
```

**Parsing rules:**
- Split on `,` (commas)
- Trim whitespace from each item
- Detect type per item:
  - Starts with `http://` or `https://` → URL
  - Is a file path that exists → Document
  - Everything else (especially quoted strings) → Description text
- Show the user what you parsed before proceeding:

```
Got it, I'll work with:
  🌐 https://mystore.com
  🌐 https://mystore.com/faq
  💬 "We do free returns within 30 days"

Fetching sources...
```

### Step 2: Ingest All Sources

Process each resource:

| Type | Action |
|---|---|
| URL | Deep crawl — see `references/crawl_strategy.md` |
| Document | `read` the file, split into sections |
| Description | Parse directly as raw context |

Combine everything into a unified knowledge base. De-duplicate content across sources.

After ingestion, show a brief summary:
```
✅ Sources ingested:
  🌐 mystore.com — 24 pages crawled (Products, FAQ, Policies, Blog)
  🌐 mystore.com/faq — merged into main crawl
  💬 Description noted: free returns within 30 days

Total: 24 knowledge pages ready.
```

### Step 3: Draft Identity

Ask the user for a short description of what the agent should be:

```
✏️  Describe this agent in one or two sentences.
   What role should it play? Who is it for?

   Example: "A friendly customer support agent for my online store,
   helping shoppers with orders, returns, and product questions."

> Identity:
```

If the user is vague, suggest roles based on the sources (see `references/agent_templates.md`):

```
Based on what I found, here are some ideas:
  1. Customer Support Agent — answer FAQs, handle returns, shipping info
  2. Shopping Assistant — help customers find products, recommend items
  3. Brand Voice Writer — create content matching the store's style

Pick one, combine, or describe your own:
```

### Step 4: Quick Confirmation

Before building, confirm the plan:

```
🔨 Ready to build:

  Name:      MyStore Support
  Role:      Customer Support Agent
  Audience:  Online shoppers
  Tone:      Friendly, helpful
  Sources:   24 pages from mystore.com + 1 description
  Skills:    navigate-knowledge, check-faq, handle-returns
  Boundaries: Cannot process refunds, cannot access accounts

  Proceed? (y/n, or tell me what to change)
```

### Step 5: Build

Generate the full agent. Every agent gets ALL of the following:

1. **`agent.json`** — Full manifest with name, id, identity, resource, environment, tools, skills, description
2. **`SYSTEM_PROMPT.md`** — Layer 1, always in context
3. **`knowledge/`** — Progressive Disclosure knowledge base
   - `index.md` — Layer 2 summary
   - `sitemap.json` — Layer 2 structure
   - `pages/` — Layer 3 full content
4. **`skills/`** — At minimum `navigate-knowledge` + 1-3 role-specific skills
5. **`tools/search_knowledge.py`** — Copy from the bundled template at
   `<shape-extension>/tools/search_knowledge.py`. If unavailable, inline the
   version in `references/skill_templates.md`.

Generate a unique ID: `<slugified-name>-<6-char-random-hex>`.

Save to `~/.bb-agent/agents/<agent-id>/`.

Update `~/.bb-agent/agents/registry.json` (create if missing — see schema below).

### Step 6: Done

```
✅ Agent created: MyStore Support (mystore-support-a1b2c3)

  📁 ~/.bb-agent/agents/mystore-support-a1b2c3/
  ├── agent.json
  ├── SYSTEM_PROMPT.md
  ├── knowledge/ (24 pages)
  ├── skills/ (3 skills)
  └── tools/

  Next:
    /shape list                → browse and activate agents
    /shape organize            → rename or remove agents
```

---

## List

Show all existing agents and let the user pick one to activate.

1. Read `~/.bb-agent/agents/registry.json` (see schema below).
2. If empty or missing: `No agents yet. Use /shape new to create one.`
3. If agents exist, display as numbered list:

```
🔷 Your Agents:

  1. MyStore Support         Customer Support        mystore.com         2025-04-07
  2. vLLM Docs Assistant     Tech Support            docs.vllm.ai        2025-04-07
  3. Alex Portfolio          Personal Assistant      text description    2025-04-06

  Enter a number to activate, or 'q' to cancel:
```

4. When the user picks one:
   - Read the agent's `SYSTEM_PROMPT.md`
   - Read the agent's `agent.json`
   - Adopt the system prompt for the current session
   - Confirm:
   ```
   ✅ Activated: MyStore Support
      Role: Customer Support Agent
      Knowledge: 24 pages | Skills: 3

      I'm now your MyStore support agent. How can I help?
   ```

---

## Organize

Manage existing agents: rename, remove, or view details.

1. Read registry, show agent list as numbered list.
2. Show actions as numbered options:

```
🗂  Organize Agents:

  1. MyStore Support         mystore-support-a1b2c3
  2. vLLM Docs Assistant     vllm-docs-assistant-7caa29
  3. Alex Portfolio          alex-portfolio-x9y8z7

  Actions:
    1. Rename an agent
    2. Remove an agent
    3. View agent details
    4. Done — exit organize

  Pick an action:
```

After the user picks an action, ask which agent (by number), then execute:

### Rename
- Ask for the new name
- Update `agent.json` name field
- Update `registry.json` name field
- Confirm: `✅ Renamed #1 to "MyStore Helper"`

### Remove
- Confirm first: `⚠️  Delete "MyStore Support" and all its data? (y/n)`
- If yes: remove the agent directory + registry entry
- Confirm: `✅ Removed "MyStore Support"`

### View Details
- Read `agent.json` and display all fields:
```
📋 Agent Details: MyStore Support

  ID:          mystore-support-a1b2c3
  Role:        Customer Support Agent
  Persona:     Friendly support rep for MyStore
  Audience:    Online shoppers
  Domain:      E-commerce
  Language:    en
  Tone:        Friendly, helpful
  Source:      https://mystore.com (24 pages crawled)
  Created:     2025-04-07
  Skills:      navigate-knowledge, check-faq, handle-returns
  Tools:       search_knowledge.py
  Boundaries:  Cannot process refunds, cannot access accounts
```

---

## Load

Quick activation without going through the list.

The extension passes any remaining text after `/shape load` as the search
term. If no term is provided, the extension opens a local prompt and asks for
one.

- Search registry by exact id/path or by case-insensitive partial name match.
- If one match: activate it (same as picking from `## List`).
- If multiple matches: show them as a numbered list and ask the user to pick.
- If no match: `No agent found matching "<input>". Use /shape list to see all agents.`

---

## Agent Manifest Format (`agent.json`)

```json
{
  "name": "Human-readable Agent Name",
  "id": "slugified-name-a1b2c3",
  "version": "1.0.0",
  "created_at": "ISO 8601",
  "source": {
    "type": "website|document|text|mixed",
    "urls": ["https://..."],
    "files": ["./path/to/file"],
    "descriptions": ["raw text..."],
    "crawled_pages": 24,
    "last_crawled": "ISO 8601"
  },
  "identity": {
    "role": "Customer Support Agent",
    "persona": "Friendly, helpful support rep",
    "language": "en",
    "tone": "friendly, professional"
  },
  "resource": {
    "knowledge_pages": 24,
    "sitemap": "knowledge/sitemap.json",
    "index": "knowledge/index.md"
  },
  "environment": {
    "target_audience": "customers",
    "domain": "e-commerce",
    "boundaries": ["cannot issue refunds"]
  },
  "tools": [
    {"name": "search_knowledge", "script": "tools/search_knowledge.py"}
  ],
  "skills": [
    {"name": "navigate-knowledge", "path": "skills/navigate-knowledge/SKILL.md"},
    {"name": "check-faq", "path": "skills/check-faq/SKILL.md"}
  ],
  "description": "One-line description of this agent"
}
```

---

## Registry Format (`registry.json`)

Single file at `~/.bb-agent/agents/registry.json`. Create it if missing with
an empty `agents` array.

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

`path` is relative to `~/.bb-agent/agents/`.

---

## System Prompt Template

Layer 1 — always in context. See `references/agent_templates.md` for role-specific patterns.

```markdown
# <Agent Name>

## Identity
You are <role> for <source>. <persona>.

## Tone & Style
<tone guidelines from user input>

## Core Responsibilities
- <derived from role + sources>

## Boundaries
- DO NOT <boundary from user>

## Knowledge Access
You have a knowledge base in `knowledge/`. Use Progressive Disclosure:
1. Check `knowledge/index.md` first
2. Read specific pages from `knowledge/pages/` as needed
3. Use `knowledge/sitemap.json` for structure
Never make up information.

## Skills
- <list skills with when to use each>
```

---

## Knowledge Base Structure

Built during ingestion, organized as Progressive Disclosure:

```
knowledge/
├── sitemap.json    # Layer 2: full structure + paths + summaries
├── index.md        # Layer 2: human-readable topic index
└── pages/          # Layer 3: full content, loaded on demand
    ├── faq.md
    ├── products/
    │   └── shoes.md
    └── ...
```

See `references/crawl_strategy.md` for deep crawl details.

---

## Skill Generation

Every agent gets `navigate-knowledge`. Additional skills picked by role.

See `references/skill_templates.md` for templates. Key mapping:

| Role | Skills |
|---|---|
| Customer Support | navigate-knowledge, check-faq, handle-policy |
| Shopping Assistant | navigate-knowledge, product-info, handle-policy |
| Tech Support | navigate-knowledge, troubleshoot, explain-concept |
| Content/Brand | navigate-knowledge, explain-concept |
| Personal Assistant | navigate-knowledge, explain-concept |
| Tutor | navigate-knowledge, explain-concept |

---

## Error Handling

- **Site blocks crawling**: note failed pages, proceed with what you got, tell the user.
- **Empty source**: tell the user, ask for more info.
- **Duplicate source URL**: ask — update existing agent or create new?
- **Large sites (100+ pages)**: pause at 50, show progress, ask to continue or limit.
- **File not found**: tell the user, skip that resource, proceed with others.
- **Registry missing**: create `~/.bb-agent/agents/registry.json` with `{"version":1,"agents":[]}`.
