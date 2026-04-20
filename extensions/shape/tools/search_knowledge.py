#!/usr/bin/env python3
"""Search across all knowledge pages in a shaped agent's knowledge base.

Usage: python search_knowledge.py <agent_path> <query>

Searches all .md files in knowledge/pages/ for the query string.
Returns matching file paths and the lines containing matches.
"""

import sys
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
            "matching_lines": matching_lines[:10],
        })

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
        for r in results[:10]:
            print(f"\n📄 {r['path']} (matched {r['score']}/{r['total_query_words']} terms)")
            for m in r["matching_lines"][:5]:
                print(f"   L{m['line_number']}: {m['text']}")
