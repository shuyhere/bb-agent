# Skill Templates

Reusable skill patterns for generated agents. When shaping an agent, pick the relevant templates, customize them for the specific source, and generate SKILL.md files.

---

## navigate-knowledge (EVERY agent gets this)

```markdown
---
name: navigate-knowledge
description: "Find information in the agent's knowledge base. Use whenever you need to look up a specific topic, answer a question, or find details. This is your primary way to access stored knowledge — use it before saying you don't know something."
---

# Navigate Knowledge

You have a structured knowledge base with Progressive Disclosure layers.

## How to find information

1. **Start with the index** — Read `knowledge/index.md` for a summary of all available pages and where to find each topic.

2. **Check the sitemap** — If you need to understand the structure or find a page by URL, read `knowledge/sitemap.json`.

3. **Read the specific page** — Once you've identified the right page from the index, read it from `knowledge/pages/<path>`.

## Rules
- Always check the index before reading individual pages — don't load pages blindly.
- If a question spans multiple topics, read the relevant pages one at a time.
- If the information isn't in any page, say so honestly. Don't make up answers.
- Quote or paraphrase from the knowledge base when answering, so the user can trust the info.
```

---

## check-faq

```markdown
---
name: check-faq
description: "Quick answers to frequently asked questions. Use when the user asks a common question about <domain> — shipping, returns, pricing, hours, etc. Check here first for fast answers before diving into deeper knowledge pages."
---

# Check FAQ

Fast path for common questions.

## How to use
1. Read `knowledge/pages/faq.md` (or the relevant FAQ page from the index)
2. Find the matching question
3. Give the answer directly, with a brief note about where the info came from

## If the FAQ doesn't cover it
Fall back to navigate-knowledge to search other pages.
```

---

## product-info

```markdown
---
name: product-info
description: "Look up product details, specifications, pricing, and availability. Use when the user asks about specific products, wants to compare items, or needs catalog information."
---

# Product Info

Access product catalog information.

## How to use
1. Check `knowledge/index.md` for the products section
2. Read the relevant product page from `knowledge/pages/products/`
3. Present details clearly: name, description, price, specs

## For comparisons
Read multiple product pages and present a side-by-side summary.

## If a product isn't found
Say so, and suggest similar products if possible based on what's in the catalog.
```

---

## handle-policy

```markdown
---
name: handle-policy
description: "Look up and explain policies — returns, shipping, privacy, terms of service, etc. Use when the user asks about rules, processes, timelines, or conditions related to doing business with <source>."
---

# Handle Policy

Quick access to policy information.

## How to use
1. Identify which policy the user is asking about
2. Read the relevant policy page from `knowledge/pages/`
3. Summarize the key points clearly — don't dump the entire policy
4. Highlight: deadlines, conditions, exceptions, how-to steps

## Important
- Always be accurate — policies are commitments. Quote when possible.
- If a policy is ambiguous, note the ambiguity rather than guessing.
- For edge cases, recommend the user contact support directly.
```

---

## troubleshoot

```markdown
---
name: troubleshoot
description: "Help users diagnose and resolve common issues. Use when the user reports a problem, error, or something not working as expected."
---

# Troubleshoot

Guide users through issue resolution.

## How to use
1. Understand the problem — ask clarifying questions if needed
2. Check `knowledge/index.md` for relevant troubleshooting or FAQ pages
3. Walk through solutions step by step
4. If the first solution doesn't work, try alternatives

## Escalation
If you can't resolve the issue from the knowledge base, tell the user and suggest:
- Contact channels (if available in knowledge)
- Information they should have ready when contacting support
```

---

## explain-concept

```markdown
---
name: explain-concept
description: "Explain concepts, features, or topics from the knowledge base. Use when the user wants to understand something, asks 'what is...', 'how does... work', or needs a concept broken down."
---

# Explain Concept

Break down topics clearly.

## How to use
1. Find the relevant page(s) in the knowledge base
2. Explain the concept at an appropriate level
3. Use examples if the source material provides them
4. Link to related topics if relevant

## Adaptation
- If the user seems technical, be precise and concise
- If the user seems non-technical, use simpler language and analogies
- If the concept is complex, break it into steps
```

---

## search_knowledge.py (tool template)

This is a Python script that searches across all knowledge pages. Bundle it in `tools/` for agents with large knowledge bases.

```python
#!/usr/bin/env python3
"""Search across all knowledge pages in a shaped agent's knowledge base.

Usage: python search_knowledge.py <agent_path> <query>

Searches all .md files in knowledge/pages/ for the query string.
Returns matching file paths and the lines containing matches.
"""

import sys
import os
import re
from pathlib import Path


def search_knowledge(agent_path: str, query: str) -> list[dict]:
    pages_dir = Path(agent_path) / "knowledge" / "pages"
    if not pages_dir.exists():
        print(f"No knowledge/pages/ directory found at {agent_path}", file=sys.stderr)
        return []

    query_lower = query.lower()
    query_words = query_lower.split()
    results = []

    for md_file in sorted(pages_dir.rglob("*.md")):
        content = md_file.read_text(encoding="utf-8", errors="replace")
        content_lower = content.lower()

        # Score: how many query words appear in the content
        score = sum(1 for w in query_words if w in content_lower)
        if score == 0:
            continue

        # Find matching lines
        matching_lines = []
        for i, line in enumerate(content.split("\n"), 1):
            if any(w in line.lower() for w in query_words):
                matching_lines.append({"line_number": i, "text": line.strip()})

        rel_path = md_file.relative_to(Path(agent_path) / "knowledge")
        results.append({
            "path": str(rel_path),
            "score": score,
            "total_query_words": len(query_words),
            "matching_lines": matching_lines[:10],  # Cap at 10 lines per file
        })

    # Sort by score descending
    results.sort(key=lambda r: r["score"], reverse=True)
    return results


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <agent_path> <query>", file=sys.stderr)
        sys.exit(1)

    agent_path = sys.argv[1]
    query = " ".join(sys.argv[2:])
    results = search_knowledge(agent_path, query)

    if not results:
        print(f"No results found for: {query}")
    else:
        for r in results[:10]:  # Show top 10 files
            print(f"\n📄 {r['path']} (matched {r['score']}/{r['total_query_words']} terms)")
            for m in r["matching_lines"][:5]:
                print(f"   L{m['line_number']}: {m['text']}")
```

---

## How to customize templates

When generating skills for a specific agent:

1. **Pick relevant templates** from above based on the agent's role (see `agent_templates.md` for which skills map to which roles)
2. **Replace placeholders** — `<domain>`, `<source>` with actual values
3. **Add source-specific details** — If the FAQ page is at `pages/help-center.md` instead of `pages/faq.md`, update the paths
4. **Adjust the description** — Make it specific to the source so it triggers correctly
5. **Add custom skills** — If the source has unique features (e.g., a restaurant with a reservation system), create a custom skill that isn't in these templates

Every agent must have at minimum:
- `navigate-knowledge` (always)
- 1-2 role-specific skills from the templates above
