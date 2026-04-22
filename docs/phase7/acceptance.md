# Repo Hyperindex Phase 7 Acceptance Contract

## Purpose

This document defines what Phase 7 must ship and what it must not ship.

If a required item below is missing, Phase 7 is not done.
If a prohibited item below ships, scope has widened past the approved slice.

## Current Closeout Note

As of 2026-04-20, the repository has:

- a checked-in Phase 2 daemon and snapshot runtime
- a checked-in Phase 4 symbol engine
- a checked-in Phase 5 impact engine
- a checked-in Phase 6 semantic engine
- no checked-in Phase 3 exact-search runtime
- no checked-in planner or planner-mode harness path

Phase 7 must build the planner over that exact reality.

## Phase 7 Must Ship

### Planner core

- A real global planner layer in the Rust workspace.
- Deterministic intent classification for one surface query plus optional explicit hints.
- A typed unified query IR.
- Deterministic route selection across exact, symbol, semantic, and impact capabilities.

### Routing, fusion, and shaping

- Explicit route budgets and fallback rules.
- Deterministic score normalization across engine outputs.
- Deterministic fusion, deduplication, and grouping.
- Deterministic no-answer and ambiguity handling.

### Evidence and traceability

- Evidence-backed planner results only.
- Structured trust payloads for planner groups.
- Machine-readable planner traces showing classification, route attempts, fallbacks, and budget
  usage.
- No planner result without concrete file or symbol provenance from an underlying engine.

### Runtime integration

- Compatible daemon protocol support for planner queries.
- CLI support for one front-door planner query command.
- Planner execution that remains snapshot-scoped and local-only.

### Harness integration

- A real planner-mode path through the existing Phase 1 harness.
- Backward-compatible use of the current run, report, and compare flow.
- Additive planner metrics only; no artifact-name churn.

### Docs

- A current execution plan.
- A current acceptance contract.
- A current decisions log for durable architecture choices.
- A current status document with commands run, risks, and the next recommended prompt.

## Phase 7 Must Preserve

### Phase 0 wedge

- local-first behavior
- TypeScript and JavaScript focus
- evidence-first answers
- the impact-analysis product wedge

### Phase 1 harness contracts

- `hyperbench` public commands
- existing query-pack and golden-set schema contracts
- normalized `QueryHit`
- current run, report, and compare artifact names
- the checked-in exact, symbol, semantic, and impact packs and goldens

### Phase 2 runtime contracts

- repo registration and repo identity flow
- snapshot creation, read-file, and diff behavior
- working-tree and buffer overlay precedence
- existing daemon transport and request/response model

### Phase 3 exact-search ownership boundary

- Phase 7 must not ship a new exact-search engine
- exact search must remain an optional route capability in the current repo
- exact-route absence must be surfaced explicitly instead of hidden behind semantic or symbol
  behavior

### Phase 4 symbol contracts

- existing `SymbolId` identity
- existing symbol query methods
- existing `GraphEdgeKind` meanings
- existing symbol build metadata used by report and compare flows

### Phase 5 impact contracts

- `ImpactAnalyzeResponse` remains the blast-radius source of truth
- impact target resolution stays within the checked-in public target kinds
- the planner must not fabricate impact results without a real impact engine call

### Phase 6 semantic contracts

- existing `SemanticQueryParams` and `SemanticQueryResponse`
- existing semantic filter model
- existing deterministic explanation signal model
- semantic retrieval remains a separate engine, not planner-owned semantic logic

## Phase 7 Must Not Ship

- a checked-in exact-search engine
- a replacement symbol, semantic, or impact engine
- LLM answer generation, prose summaries, or answer cards
- learned routing, learned fusion, or LLM / cross-encoder reranking
- codemods, auto-refactors, or write-side editor actions
- editor UI, answer-generation UI, browser UI, or dashboard work
- VS Code extension product work
- cloud sync, team-sharing, or multi-user runtime behavior
- backward-incompatible query-pack schema changes
- backward-incompatible run, report, or compare artifact changes
- planner logic that reads repo files outside the snapshot model

## Acceptance Checklist

Phase 7 is acceptable only if every item below can be answered `yes`.

### Planner behavior

- Can the planner classify lookup, semantic, and impact intent deterministically?
- Can it build one typed IR before route execution?
- Can it choose bounded routes with explicit fallback behavior?

### Evidence and trust

- Does every planner result map back to concrete engine evidence?
- Are trust payloads deterministic and template-based?
- Are ambiguity and no-answer cases explicit rather than guessed?

### Runtime and CLI

- Does the daemon expose a compatible planner query method?
- Does the CLI expose a usable front-door planner query command?
- Is planner execution still repo- and snapshot-scoped?

### Harness behavior

- Does the planner run through the existing harness as a backward-compatible extension?
- Are current artifact names preserved?
- Are planner metrics additive rather than schema-breaking?

### Scope control

- Is exact search still unshipped in the current repo?
- Are symbol, impact, and semantic engines still preserved as the source of truth for their own
  domains?
- Are UI, extension, codemod, cloud, and team-sharing features still unshipped?

## Minimum Evidence For Phase Completion

The final Phase 7 closeout should include:

- targeted Rust test results for planner classification, IR, routing, fusion, and daemon transport
- compatibility validation showing Phase 2, Phase 4, Phase 5, and Phase 6 behavior still works
- planner-mode harness validation through `hyperbench`
- measured planner latency and accuracy metrics from the checked-in benchmark flow
- updated Phase 7 execution, acceptance, decisions, and status docs
- explicit confirmation that the current repo still does not ship an exact-search engine
