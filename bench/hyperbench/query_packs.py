"""Query pack loading and cross-artifact validation helpers."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from hyperbench.schemas import GoldenSet, QueryPack, QueryType

QUERY_PACK_DIR = Path("bench/configs/query-packs")
GOLDEN_SET_DIR = Path("bench/configs/goldens")
MANUAL_PLACEHOLDER_PATH = "__manual_verification_required__"
SYNTHETIC_TARGET_COUNTS: dict[QueryType, int] = {
    QueryType.EXACT: 100,
    QueryType.SYMBOL: 50,
    QueryType.SEMANTIC: 30,
    QueryType.IMPACT: 30,
}


class QueryArtifactValidationError(ValueError):
    """Raised when query pack and golden artifacts do not line up."""


@dataclass(frozen=True)
class QueryArtifactSummary:
    """Summary of validated query-pack and golden-set artifacts."""

    query_pack_count: int
    golden_set_count: int
    query_counts: dict[QueryType, int]


def load_query_artifacts(
    query_pack_dir: Path | str = QUERY_PACK_DIR,
    golden_set_dir: Path | str = GOLDEN_SET_DIR,
) -> tuple[list[QueryPack], list[GoldenSet]]:
    """Load every query pack and golden set from the configured directories."""
    pack_root = Path(query_pack_dir)
    golden_root = Path(golden_set_dir)
    return (
        [QueryPack.from_path(path) for path in _iter_schema_paths(pack_root)],
        [GoldenSet.from_path(path) for path in _iter_schema_paths(golden_root)],
    )


def validate_query_artifacts(
    query_packs: list[QueryPack],
    golden_sets: list[GoldenSet],
    *,
    synthetic_corpus_ids: set[str] | None = None,
    required_synthetic_counts: dict[QueryType, int] | None = None,
) -> QueryArtifactSummary:
    """Validate query-pack/golden-set pairs and synthetic query-count coverage."""
    errors: list[str] = []
    pack_ids = [pack.query_pack_id for pack in query_packs]
    golden_ids = [golden.golden_set_id for golden in golden_sets]
    if len(pack_ids) != len(set(pack_ids)):
        errors.append("query packs must not contain duplicate query_pack_id values")
    if len(golden_ids) != len(set(golden_ids)):
        errors.append("golden sets must not contain duplicate golden_set_id values")

    packs_by_id = {pack.query_pack_id: pack for pack in query_packs}
    goldens_by_pack: dict[str, list[GoldenSet]] = {}
    for golden in golden_sets:
        goldens_by_pack.setdefault(golden.query_pack_id, []).append(golden)

    for pack in query_packs:
        matching_goldens = goldens_by_pack.get(pack.query_pack_id, [])
        if not matching_goldens:
            errors.append(
                f"query pack '{pack.query_pack_id}' is missing a matching golden set"
            )
            continue
        if len(matching_goldens) > 1:
            errors.append(
                f"query pack '{pack.query_pack_id}' has multiple matching golden sets"
            )
            continue
        golden = matching_goldens[0]
        if golden.corpus_id != pack.corpus_id:
            errors.append(
                f"golden set '{golden.golden_set_id}' corpus_id does not match "
                f"query pack '{pack.query_pack_id}'"
            )
        pack_query_ids = {query.query_id for query in pack.queries}
        expectation_ids = {expectation.query_id for expectation in golden.expectations}
        missing = sorted(pack_query_ids - expectation_ids)
        if missing:
            errors.append(
                f"golden set '{golden.golden_set_id}' is missing expectations for: "
                + ", ".join(missing)
            )
        dangling = sorted(expectation_ids - pack_query_ids)
        if dangling:
            errors.append(
                f"golden set '{golden.golden_set_id}' has expectations for unknown queries: "
                + ", ".join(dangling)
            )

    missing_packs = sorted(
        golden.query_pack_id
        for golden in golden_sets
        if golden.query_pack_id not in packs_by_id
    )
    if missing_packs:
        errors.append(
            "golden sets reference unknown query_pack_id values: " + ", ".join(missing_packs)
        )

    query_counts: dict[QueryType, int] = {query_type: 0 for query_type in QueryType}
    synthetic_ids = synthetic_corpus_ids or set()
    for pack in query_packs:
        if synthetic_ids and pack.corpus_id not in synthetic_ids:
            continue
        for query in pack.queries:
            query_counts[QueryType(query.type)] += 1

    targets = required_synthetic_counts or {}
    for query_type, minimum in targets.items():
        if query_counts.get(query_type, 0) < minimum:
            errors.append(
                f"synthetic query coverage for {query_type.value} is "
                f"{query_counts.get(query_type, 0)}, below required minimum {minimum}"
            )

    if errors:
        raise QueryArtifactValidationError("\n".join(errors))

    return QueryArtifactSummary(
        query_pack_count=len(query_packs),
        golden_set_count=len(golden_sets),
        query_counts=query_counts,
    )


def _iter_schema_paths(root: Path) -> list[Path]:
    if not root.exists():
        return []
    return sorted(
        path
        for path in root.iterdir()
        if path.is_file() and path.suffix.lower() in {".json", ".yaml", ".yml"}
    )
