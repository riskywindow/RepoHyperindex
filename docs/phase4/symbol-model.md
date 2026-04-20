# Repo Hyperindex Phase 4 Symbol Model

This document defines the Phase 4 symbol data model behind the public transport contract.

The intent is a deterministic, syntax-derived model that is useful for:

- symbol search
- show by id
- definitions
- references
- location-based resolution

The intent is not to model full compiler semantics.

## Identity

### `symbol_id`

`symbol_id` identifies the logical declaration-level symbol.

Properties:

- stable enough to query and persist within Phase 4
- can survive multiple occurrences in one snapshot
- should be derived from deterministic declaration facts, not request order

Current Phase 4 recipe:

- hash repo id, file path, symbol kind, container chain, declared name, and a normalized
  declaration-signature digest
- expose the hash behind a readable prefix:
  - `sym.<kind>.<sanitized-name>.<digest12>`
- treat the synthetic per-file `module` symbol as the root container for top-level declarations

Current normalized declaration-signature inputs:

- `module`
  - file path plus language id
- `function` / `method` / `constructor`
  - normalized parameter text plus normalized return-type text
- `class`
  - normalized heritage clauses only
- `interface` / `type_alias` / `enum`
  - normalized declaration text
- `import_binding`
  - module specifier, imported name, and local name

This intentionally ignores whitespace and nearby formatting edits, but it does not promise
continuity across real renames, moves, or declaration-shape changes.

### `occurrence_id`

`occurrence_id` identifies one concrete occurrence of a symbol.

Properties:

- snapshot-local
- precise enough to anchor definitions and references
- expected to change more often than `symbol_id`

Current Phase 4 recipe:

- hash snapshot id, file path, symbol id, occurrence role, and occurrence span
- expose the hash behind a readable prefix:
  - `occ.<role>.<digest12>`

## Source Locations

`SourceSpan` contains:

- `start { line, column }`
- `end { line, column }`
- `bytes { start, end }`

Rules:

- lines and columns are 1-based
- byte ranges are half-open
- every occurrence and symbol record should carry a span
- spans should refer to the snapshot-resolved file content, not the live working tree outside the snapshot

## Languages

The Phase 4 contract recognizes only the TS/JS family:

- `typescript`
- `tsx`
- `javascript`
- `jsx`
- `mts`
- `cts`

If more languages are added later, they should arrive as new enum variants plus a new language-pack
config entry, not as free-form strings.

## Symbol Kinds

The current public symbol kinds are:

- `file`
- `module`
- `namespace`
- `class`
- `interface`
- `type_alias`
- `enum`
- `enum_member`
- `function`
- `method`
- `constructor`
- `property`
- `field`
- `variable`
- `constant`
- `parameter`
- `import_binding`

These kinds are intentionally syntax-oriented. They do not imply runtime behavior or type-system
meaning beyond what the extractor can prove.

## Occurrence Roles

An occurrence carries one of:

- `definition`
- `declaration`
- `reference`
- `import`
- `export`

Phase 4 should prefer evidence over inference:

- use `definition` when the extractor can point to the declaration anchor
- use `reference` for concrete identifier uses
- use `import` and `export` when the occurrence participates directly in those syntactic forms

## File Facts

`FileFacts` is the contract-level per-file fact bundle.

It contains:

- `symbols`
- `occurrences`
- `edges`
- `diagnostics`

This is the narrowest useful shape for Phase 4 because it supports:

- parse inspection
- debug visibility
- durable persistence
- future query evaluation without forcing tree exposure

The current extractor also keeps internal persistence metadata per symbol:

- container symbol id
- visibility state:
  - local
  - exported
  - default_export
- file ownership path
- declaration-signature digest

## Graph Edge Kinds

The graph is intentionally limited to deterministic, syntax-derived edges:

- `contains`
- `defines`
- `references`
- `imports`
- `exports`

Out of scope for this model:

- inferred call graphs
- type inheritance reasoning
- override detection
- alias chasing across compiler semantics
- control-flow or data-flow edges

## Diagnostics

Diagnostics are part of the model because Phase 4 may produce partial facts while still remaining
queryable.

Current severities:

- `error`
- `warning`
- `info`

Current diagnostic codes:

- `syntax_error`
- `unsupported_language`
- `unsupported_syntax`
- `truncated_input`
- `partial_analysis`
- `duplicate_fact`

Guidance:

- use `partial_analysis` when the engine intentionally skips behavior that is documented as future work
- diagnostics should be tied to a `path` and `span` when possible
- diagnostics should not silently downgrade into missing facts

Current extractor rule:

- prefer a warning/info diagnostic over a speculative fact when syntax is unsupported or ambiguous
- on broken-but-editing-relevant files, recover only explicit export shapes that remain directly
  visible in the snapshot text

## Resolution Model

`symbol_resolve` is intentionally location-first, not semantic.

Supported selectors:

- `line_column`
- `byte_offset`

Resolution should return:

- the resolved `SymbolRecord`
- optionally the exact `SymbolOccurrence` used as evidence

Current library behavior:

- prefer the smallest containing occurrence span when a cursor falls directly on definition,
  import, export, or reference evidence
- otherwise fall back to the smallest containing symbol span in the file
- for imported identifiers, resolution returns the local import-binding symbol; canonical
  definition lookup then follows exact import/export alias edges to the upstream declaration

If the engine cannot resolve a symbol at a location, return no resolution or a typed error. Do not
invent a best-effort semantic answer.

## Search Model

`symbol_search` is name-based in this slice.

The request includes:

- `text`
- `mode`
- `kinds`
- `path_prefix`

The current search modes are:

- `exact`
- `prefix`
- `substring`

Current ranking is deterministic and explanation-first:

1. stronger name match:
   - exact before prefix before substring
2. exact-case match preference when available
3. requested symbol-kind preference in query order when `kinds` is supplied
4. exported/default-export symbols before local-only symbols
5. filename-stem match when it cheaply agrees with the query text
6. shallower container depth before deeper nesting
7. deterministic tie-breaks by repo-relative path, declaration position, and `symbol_id`

Each ranked result carries:

- the public `SymbolSearchHit`
- a reason string summarizing the positive ranking factors
- a structured explanation payload describing the score components and tie-break inputs

Phase 4 search is not:

- fuzzy semantic search
- ranking by embeddings
- whole-program relevance scoring

## Stable Semantics Boundary

This model promises only what the Phase 4 implementation should actually build:

- syntax-derived declarations
- syntax-derived occurrences
- deterministic spans
- deterministic persistence-ready graph edges

It does not promise:

- compiler-grade bindings
- perfect cross-file symbol continuity through arbitrary refactors
- typechecker-grade definitions and references

## Current Extraction Boundary

Currently extracted:

- one synthetic `module` symbol per supported file
- declarations for:
  - functions
  - classes
  - methods
  - constructors
  - interfaces
  - type aliases
  - enums
  - variable-bound functions
  - import bindings
- direct export evidence for:
  - exported declarations
  - local export clauses
  - named re-export aliases
- conservative identifier-reference linkage for:
  - plain identifier and type-identifier uses the extractor can bind to an owned symbol or import
    binding without compiler semantics
- repo-local module resolution for:
  - relative specifiers
  - direct workspace package-name imports backed by in-snapshot `package.json` `name` fields
- conservative broken-file recovery for:
  - `export default function`
  - `export default class`
  - `export const|let|var <name> = ...` function-shaped bindings

Explicitly unsupported in this slice:

- wildcard re-exports
- computed method names
- destructuring declaration bindings
- Node `exports` maps or package-manager install layouts
- `tsconfig` / `jsconfig` path aliases
- test files / test blocks
- framework-specific secondary facts
- non-declaration default-export expressions
- property-name, string-key, or runtime-reflection references
