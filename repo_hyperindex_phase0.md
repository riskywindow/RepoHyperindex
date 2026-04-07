# Repo Hyperindex Phase 0 Package

## Public one-sentence positioning

Repo Hyperindex is a local-first TypeScript impact engine that indexes live code edits and tells you exactly where behavior lives and what will break before you change it.

---

## 1) Product brief / PRD (2-page version)

### Product name
Repo Hyperindex

### V1 wedge
TypeScript local impact engine.

### Summary
Repo Hyperindex v1 is a local-only developer tool for medium-to-large TypeScript codebases. It combines fast exact search, symbol-aware navigation, semantic retrieval, and impact analysis into one workflow. The core promise is not "better code search." The core promise is: **find the relevant behavior in your current local branch, including unsaved edits, and understand the blast radius of a change before you make it.**

### Problem
Developers lose time in large TypeScript repositories for three reasons:
1. They cannot quickly find where a behavior actually lives.
2. They can find code, but they cannot quickly determine what else depends on it.
3. Their local working copy is ahead of hosted indexing, so search and AI answers are often stale right when confidence matters most.

Hosted tools have improved quickly. GitHub documents that Copilot repository indexing is automatically updated within seconds of starting a new conversation after an index exists, and GitHub's March 2025 changelog says semantic code search indexing now completes in seconds in most cases. GitHub also documents that browser code navigation only works on active branches and repositories with fewer than 100,000 files. Sourcegraph documents that its most reliable precise code navigation relies on SCIP indexing and often recommends running indexers in CI for complex builds. These are strong products, but they leave a clear opening for a local-first tool optimized for working-tree freshness and impact analysis rather than hosted repository understanding. [1][2][3][4]

### Why now
The market no longer rewards "semantic code search" by itself. Hosted semantic indexing and code navigation already exist. The differentiated opportunity is a **live local model** of the repository. Tree-sitter is specifically designed for incremental parsing and efficient updates as files are edited, and GitHub's stack graphs work shows that config-free, incrementally processed code intelligence is a viable approach for code navigation without forcing per-repository CI setup. [5][6]

### Target user
Primary:
- Senior and mid-level engineers working in TypeScript/JavaScript codebases from roughly 250k to 3M LOC.
- Developers onboarding into unfamiliar services or monorepos.
- Engineers performing risky refactors involving auth, data models, config, routing, or shared libraries.

Secondary:
- Staff engineers reviewing architecture and dependency boundaries.
- AI-assisted developers who need a trusted context pack before handing work to an agent.

### Jobs to be done
When I need to change behavior in a large TypeScript codebase,
- help me find the canonical implementation fast,
- show me the symbols and files that support that answer,
- tell me what else is coupled to it,
- and update that view instantly as I edit my local branch,
so I can make changes with confidence and run the right tests.

### Core value proposition
Repo Hyperindex wins if it reduces "question to safe change" time.

The main product loop is:
1. Ask a code question.
2. Jump to the exact implementation.
3. Inspect supporting symbols and references.
4. Click for blast radius.
5. Edit locally and watch the result update.

### Product principles
- Local-first: local branch and unsaved buffers are first-class inputs.
- Evidence-first: every answer is backed by files, symbols, and graph edges.
- Fast before clever: exact and symbol search must feel instant.
- Incremental by default: edits update the relevant indexes without full reindex.
- Trustworthy impact: distinguish certain, likely, and possible downstream effects.
- No mandatory build or CI: v1 must work without project-specific indexing pipelines.

### In-scope for v1
- One local repository at a time.
- TypeScript/JavaScript only.
- macOS and Linux.
- VS Code extension plus CLI.
- Exact search.
- Symbol search/navigation.
- Semantic retrieval over code chunks.
- Impact analysis for files, symbols, routes, and config keys.
- Live updates for branch changes and unsaved buffers.

### Out-of-scope for v1
- Multi-repo search.
- Team-shared cloud indexes.
- General-purpose chatbot UI.
- Cross-language precision beyond TS/JS.
- Full codemod/refactor engine.
- Enterprise auth/permissions.
- CI-based indexing requirements.
- Full compiler-grade semantic correctness for every TS edge case.

### Success metrics
Product success:
- A developer can answer "where does this behavior live?" in less than 30 seconds.
- A developer can answer "what breaks if I change this?" in less than 60 seconds.
- The blast-radius view is trustworthy enough to guide code review and test selection.

Performance success:
- Exact query p95 under 50 ms warm.
- Symbol query p95 under 75 ms warm.
- Impact query p95 under 300 ms warm.
- Semantic query p95 under 200 ms warm after embeddings are ready.
- Exact/symbol refresh under 200 ms after a single-file edit.
- Semantic refresh under 2 s after a single-file edit.

### Launch thesis
The launch story is not "AI for code search."
The launch story is:
**"Your hosted tools understand the repo. Repo Hyperindex understands the code you are editing right now, and shows the blast radius before you commit."**

---

## 2) Exact v1 wedge

**TypeScript local impact engine**

Not:
- a general AI IDE,
- a hosted code search platform,
- a Sourcegraph clone,
- a Copilot competitor,
- a compiler research project.

Specifically:
- Local daemon
- TypeScript/JavaScript only
- Live branch + buffer overlays
- Exact + symbol + semantic + impact
- Primary hero feature: blast radius for a change

This wedge exists because hosted tools already solve large parts of search and semantic retrieval, while local freshness and change-impact remain poorly served in one tight workflow. GitHub's docs emphasize repository-level indexing and active-branch limitations, and Sourcegraph's docs emphasize SCIP/CI for its most precise path. Repo Hyperindex v1 should attack the gap between "I found code" and "I know this change is safe." [1][2][3][4]

---

## 3) Query contract

Canonical query envelope:

```json
{
  "repo_id": "local repo id",
  "snapshot": {
    "base": "git commit or working tree id",
    "include_working_tree": true,
    "buffers": [
      {
        "uri": "file:///path/to/file.ts",
        "version": 12,
        "contents": "optional unsaved buffer text"
      }
    ]
  },
  "type": "exact | symbol | semantic | impact",
  "limit": 20,
  "query": {}
}
```

### A. Exact query
Purpose:
- Find exact text, regex matches, paths, and scoped matches quickly.

Accepted forms:
- raw text query
- regex
- path filter
- file-type filter
- package/path glob filter

Example:
```json
{
  "type": "exact",
  "query": {
    "text": "invalidateSession",
    "mode": "plain",
    "path_globs": ["packages/auth/**"],
    "languages": ["typescript", "tsx"]
  }
}
```

Returns:
- ranked list of matches
- file path, line range, preview
- reason for match (text/path/regex)
- optional nearby symbol

Latency target:
- p95 under 50 ms warm

### B. Symbol query
Purpose:
- Navigate identifiers, definitions, references, containers, callers, and related tests.

Accepted forms:
- symbol string
- fully-qualified symbol id
- cursor selection in editor

Example:
```json
{
  "type": "symbol",
  "query": {
    "symbol": "invalidateSession",
    "scope": "repo"
  }
}
```

Returns:
- canonical definition(s)
- references
- caller/callee edges when available
- enclosing module/package
- tests associated with touched symbols/files

Latency target:
- p95 under 75 ms warm

### C. Semantic query
Purpose:
- Answer natural-language "where/how" questions when the user does not know the exact identifier.

Accepted forms:
- natural language string
- optional scope filters (path/package/language)
- optional rerank mode

Example:
```json
{
  "type": "semantic",
  "query": {
    "text": "where do we invalidate sessions?",
    "path_globs": ["packages/**"]
  }
}
```

Returns:
- ranked evidence hits (functions/classes/modules)
- optional 1-paragraph answer card generated only from cited evidence
- supporting symbols and files
- confidence score and reason codes

Rules:
- No answer card without evidence hits.
- Retrieval is hybrid: lexical + symbol + semantic reranking.
- Semantic indexing is background and progressive.

Latency target:
- p95 under 200 ms warm after embeddings are ready

### D. Impact query
Purpose:
- Tell the user what is likely to break or require retesting if a file/symbol/route/config key changes.

Accepted forms:
- symbol id
- file path
- route id
- config key
- optional change hint (rename/signature change/delete)

Example:
```json
{
  "type": "impact",
  "query": {
    "target_type": "symbol",
    "target": "packages/auth/src/session/service.ts#invalidateSession",
    "change_hint": "modify behavior"
  }
}
```

Returns:
- certain impacts:
  - direct references
  - direct imports/exports
  - exact route/config bindings
- likely impacts:
  - reverse call graph neighbors
  - package dependents
  - related tests
- possible impacts:
  - semantically similar implementations
  - heuristically related config/docs
- each result includes a reason path

Latency target:
- p95 under 300 ms warm

---

## 4) Non-goals and cut list

### Non-goals
- Be the best general-purpose code search engine on the internet.
- Replace GitHub, Sourcegraph, or VS Code's native navigation.
- Solve all static analysis for TypeScript.
- Build a general coding agent or autonomous programmer.
- Offer cloud collaboration in v1.
- Provide automatic large-scale refactors in v1.
- Support every language in v1.

### Cut list (ruthless)
If schedule or complexity expands, cut these first:
1. JetBrains support
2. Windows support
3. generated-code awareness beyond basic ignore rules
4. project-wide answer summaries
5. graph visualization beyond a simple dependency panel
6. route/config-specific extractors beyond the top 1-2 frameworks
7. semantic answer card generation
8. any cloud sync or telemetry dashboard
9. multi-repo graph stitching
10. codemod execution

---

## 5) Benchmark spec

### Purpose
The benchmark suite exists to prove two things:
1. Repo Hyperindex is fast enough to feel interactive.
2. Incremental local updates are the real product differentiator.

### Primary benchmark hardware target
**Primary laptop target**
- 14-inch MacBook Pro with Apple M4 Pro
- 24 GB unified memory
- 1 TB SSD

Apple documents this as a tested/configurable 14-inch MacBook Pro M4 Pro setup, with higher-memory options also available. We are choosing it as the primary benchmark target because it is a realistic "serious solo builder / senior dev laptop" and not a workstation-class outlier. [7]

**Secondary desktop target**
- Linux workstation
- 8 CPU cores minimum
- 64 GB RAM
- NVMe SSD

**Stretch / floor target**
- Older Apple Silicon or x86 laptop with 16 GB RAM
- Used for usability regression checks, not headline numbers

### OS targets
- Primary: macOS
- Secondary: Ubuntu LTS on x86_64

### Repo size tiers
Use both real public repositories and a curated synthetic SaaS monorepo fixture.

Tier S:
- 50k to 150k LOC
- 1 to 3 packages
- exact/symbol smoke tests

Tier M:
- 250k to 750k LOC
- 5 to 20 packages
- main development benchmark

Tier L:
- 1M to 3M LOC
- 20 to 100 packages
- hero benchmark target

Tier XL (stretch):
- 3M to 10M LOC
- 100+ packages
- stress and memory profiling only

### Candidate benchmark corpora
- Real public TS/JS repositories such as VS Code, TypeScript, or Next.js for credibility and repeatability. [8][9]
- One curated public demo fixture that models a modern SaaS TypeScript monorepo with auth, session, API, worker, and test packages so the hero demo can show meaningful impact analysis.

### Metrics to capture
Indexing:
- cold start index time
- time until exact search ready
- time until symbol search ready
- time until semantic search ready
- index size on disk
- peak RSS during indexing

Queries:
- p50/p95 latency for exact, symbol, semantic, impact
- result accuracy on golden queries
- ranking quality for top-5 results

Incremental:
- latency after single-file edit
- latency after symbol rename within one package
- branch-switch refresh latency
- unsaved-buffer refresh latency

Correctness:
- full reindex vs incremental equivalence
- stale result rate
- missing-direct-reference rate for impact analysis

### Benchmark gates for shipping v1
- Exact and symbol indexes available before semantic indexing completes.
- Single-file edit reflected in exact/symbol results in under 200 ms.
- Unsaved buffer overlay reflected without saving to disk.
- Impact query returns reason paths for at least 90% of direct-reference hits in the benchmark goldens.

---

## 6) Architecture note

### V1 architectural thesis
Build a local daemon that maintains a live repository model composed of:
- a base snapshot,
- a working-tree overlay,
- and an unsaved-buffer overlay.

Queries should always execute against that composed view rather than against the last saved or hosted index.

### Why this architecture
Tree-sitter is explicitly built for incremental parsing and efficient syntax tree updates as files are edited. GitHub's stack graphs writeup shows a practical path toward config-free, incrementally processed code intelligence without requiring a build or CI job for baseline usefulness. Sourcegraph's SCIP model is the later upgrade path when we want to import build-derived precision. [5][6][4]

### System boundaries
Client surfaces:
- VS Code extension
- CLI

Core local service:
- Rust daemon
- local IPC protocol

Indexing pipeline:
1. file watcher receives disk changes
2. VS Code extension streams unsaved buffers
3. snapshot manager composes base + overlays
4. parser/extractor updates TS/JS facts
5. exact index updates changed files
6. symbol graph updates changed symbols and edges
7. semantic chunker re-embeds only changed chunks
8. planner serves queries

### Core subsystems
1. **Snapshot manager**
   - immutable base snapshot
   - working-tree overlay
   - buffer overlay

2. **Exact index**
   - trigram/inverted index for text and regex
   - path/package/language filters

3. **Parser/extractor**
   - Tree-sitter TS/JS parsers
   - symbol, import/export, route, test, config extraction

4. **Symbol graph**
   - defines, references, imports, exports, calls, contains, tested-by edges

5. **Semantic index**
   - symbol-level chunking
   - hybrid retrieval
   - background embedding pipeline

6. **Impact engine**
   - direct reference expansion
   - reverse dependency traversal
   - test impact ranking
   - certain / likely / possible tiers

7. **Query planner**
   - route the query to exact/symbol/semantic/impact
   - fuse scores
   - return evidence-backed results

### Why it should feel different
GitHub already has fast hosted indexing and browser navigation, but its code navigation is tied to active branches and file-count limits in the browser experience. Sourcegraph's most precise path is build-derived indexing. Repo Hyperindex v1 is intentionally optimized for the local developer loop: current branch, dirty files, and unsaved editor state. [1][2][3][4]

---

## 7) Hero demo and north-star launch script

### Decision
The launch hero demo should use a **curated public TypeScript SaaS monorepo fixture** rather than depending solely on a random OSS repo. Reason: the product needs a crystal-clear "blast radius" story around auth/session behavior, and a curated fixture guarantees that the query "where do we invalidate sessions?" produces a compelling, legible result. Real public repos should still be used for benchmark credibility.

### Hero demo script (90 seconds)
1. Open the curated large TypeScript monorepo in VS Code.
2. Open Repo Hyperindex side panel.
3. Type: `where do we invalidate sessions?`
4. Show semantic results grouped by symbol:
   - `invalidateSession`
   - `revokeAllUserSessions`
   - `AuthSessionService`
5. Click the top result.
6. Jump to the canonical implementation in `packages/auth/src/session/service.ts`.
7. Show supporting evidence:
   - references from logout endpoint
   - worker job that revokes sessions after password reset
   - tests covering invalidation behavior
8. Click **Blast Radius**.
9. Show impact panel with:
   - direct callers
   - reverse imports
   - affected API route
   - related Redis/session store code
   - ranked tests to run
10. Edit the function body in the unsaved buffer.
11. Without saving, show the impact panel and symbol results update.
12. Close with the line:
   - "Repo Hyperindex understands the code you are editing right now, and shows what breaks before you commit."

### North-star demo acceptance criteria
- Large TS repo opens and indexes progressively.
- Exact and symbol search are usable before semantic finishes.
- Query returns a correct implementation and supporting symbols.
- Blast radius shows direct and likely downstream effects.
- Unsaved edit changes results without saving.
- The entire flow completes in under 90 seconds on benchmark hardware.

---

## 8) Done-when checklist

Phase 0 is complete when:
- We can describe v1 in one sentence.
- We know exactly what we are not building.
- The v1 wedge is narrowed to "TypeScript local impact engine."
- The query contract is explicit and testable.
- Benchmark hardware and repo tiers are defined.
- The hero demo is chosen and scripted.
- The architecture note is clear enough to drive Phase 1 implementation.

---

## 9) Final one-sentence description

**Repo Hyperindex is a local-first TypeScript impact engine that incrementally indexes live edits and tells you exactly where behavior lives and what will break before you change it.**

---

## Sources
[1] GitHub Docs - Indexing repositories for GitHub Copilot: https://docs.github.com/en/copilot/concepts/context/repository-indexing
[2] GitHub Changelog - Instant semantic code search indexing now generally available for GitHub Copilot: https://github.blog/changelog/2025-03-12-instant-semantic-code-search-indexing-now-generally-available-for-github-copilot/
[3] GitHub Docs - Navigating code on GitHub: https://docs.github.com/en/repositories/working-with-files/using-files/navigating-code-on-github
[4] Sourcegraph Docs - Precise Code Navigation: https://sourcegraph.com/docs/code-navigation/precise-code-navigation
[5] Tree-sitter docs - Introduction: https://tree-sitter.github.io/tree-sitter/
[6] GitHub Blog - Introducing stack graphs: https://github.blog/open-source/introducing-stack-graphs/
[7] Apple Support - MacBook Pro (14-inch, M4 Pro or M4 Max, 2024) Tech Specs: https://support.apple.com/en-us/121553
[8] GitHub - microsoft/vscode: https://github.com/microsoft/vscode
[9] GitHub - vercel/next.js: https://github.com/vercel/next.js/
