# Crawl Strategy

Detailed instructions for deep-crawling a website and building a Progressive Disclosure knowledge base.

## Step 1: Initial Fetch

```
homepage = web_fetch(url)
if homepage is empty or blocked:
    homepage = browser_fetch(url)
```

Extract from the homepage:
- **Site name** — from `<title>`, `<meta og:site_name>`, or the visible header
- **Description** — from `<meta name="description">` or first paragraph
- **Language** — from `<html lang="">` or content detection
- **Navigation links** — these are the highest-priority pages to crawl

## Step 2: Build the Crawl Queue

Parse the homepage content for internal links. An internal link is any URL that:
- Shares the same domain as the input URL
- Is a relative path (e.g., `/about`, `./products`)

**Priority order for the queue:**
1. Navigation/menu links (highest — these define the site structure)
2. Footer links (often contain policy pages, contact, sitemap)
3. In-content links from the homepage
4. Links discovered on subsequent pages

**Exclude from queue:**
- Anchors to the same page (`#section`)
- Asset links (`.css`, `.js`, `.png`, `.jpg`, `.svg`, `.ico`, `.woff`)
- External links (different domain)
- Login/auth pages (`/login`, `/signin`, `/register`, `/oauth`)
- Duplicate URLs (normalize: strip trailing `/`, strip `?utm_*` params)

## Step 3: Breadth-First Crawl

Process the queue level by level:

```
Level 0: Homepage (already fetched)
Level 1: All links found on homepage
Level 2: New links found on Level 1 pages
Level 3: New links found on Level 2 pages
...continue until queue is empty or practical limit reached
```

For each page:
1. Fetch with `web_fetch` (fall back to `browser_fetch`)
2. Extract the **main content** — strip nav, footer, sidebars, ads if possible
3. Convert to clean markdown
4. Extract a **title** (from `<h1>`, `<title>`, or first heading)
5. Write a **one-line summary** of the page content
6. Extract any new internal links, add unseen ones to the queue
7. Save to `knowledge/pages/<slugified-path>.md`

**Naming convention for pages:**
- `https://example.com/faq` → `pages/faq.md`
- `https://example.com/products/shoes` → `pages/products/shoes.md`
- `https://example.com/` → `pages/home.md`
- Handle duplicates by appending `-2`, `-3`, etc.

## Step 4: Rate Limiting & Politeness

- Wait briefly between requests — don't hammer the server
- If you get rate-limited (429) or server errors (5xx), back off and retry once
- If a page returns 404, skip it and note it
- If a page requires authentication, skip it and note it

## Step 5: Build the Sitemap

After crawling, construct `sitemap.json`:

```json
{
  "root": "https://example.com",
  "total_pages": 34,
  "crawl_depth": 3,
  "pages": [
    {
      "path": "pages/home.md",
      "title": "Home",
      "url": "https://example.com/",
      "summary": "Main landing page with hero banner and featured products",
      "depth": 0,
      "children": ["pages/faq.md", "pages/products/shoes.md"]
    },
    {
      "path": "pages/faq.md",
      "title": "FAQ",
      "url": "https://example.com/faq",
      "summary": "Frequently asked questions about shipping, returns, sizing",
      "depth": 1,
      "children": []
    }
  ]
}
```

## Step 6: Build the Index

Create `index.md` — a human-readable table of contents:

```markdown
# Knowledge Index for <Site Name>

> Source: <url>
> Pages crawled: <count>
> Last updated: <date>

## Site Structure

### Main Sections
- **Home** — Landing page, featured content → `pages/home.md`
- **FAQ** — Shipping, returns, sizing questions → `pages/faq.md`
- **Products** — Full product catalog
  - Shoes → `pages/products/shoes.md`
  - Accessories → `pages/products/accessories.md`

### Policies
- **Return Policy** — 30-day window, conditions → `pages/returns.md`
- **Privacy Policy** — Data handling practices → `pages/privacy.md`

### Other
- **Blog** — Style guides, new arrivals → `pages/blog.md`
- **Contact** — Support channels, hours → `pages/contact.md`
```

Group pages by logical sections. Use the site's own navigation structure as a guide.

## Step 7: Content Quality

When saving page content to markdown:
- Preserve headings hierarchy (h1, h2, h3...)
- Keep lists, tables, and structured data
- Remove: navigation bars, cookie banners, footer boilerplate, "related articles" sections, social media widgets
- Preserve: product details, prices, specifications, FAQ answers, policy text, instructions
- If a page is mostly images with little text, note that: `> This page is primarily visual with minimal text content.`
- If a page has forms, note the form purpose: `> This page contains a contact form with fields: name, email, message.`

## Handling Large Sites

If the site has more than 100 discoverable pages:
1. After crawling 50 pages, pause and tell the user what you've found so far
2. Ask if they want to continue or if the current coverage is sufficient
3. Show which sections have been covered and which remain
4. Let them prioritize: "Focus on the product pages" or "Skip the blog"

If the site has more than 300 pages, recommend focusing on the most important sections rather than crawling everything.
