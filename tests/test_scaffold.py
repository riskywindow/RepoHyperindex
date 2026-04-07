"""Scaffold import tests for the Phase 1 Hyperbench package."""

from __future__ import annotations

import hyperbench.adapter
import hyperbench.compare
import hyperbench.corpora
import hyperbench.metrics
import hyperbench.report
import hyperbench.runner
import hyperbench.schemas
import hyperbench.synth


def test_schema_module_exports_core_models() -> None:
    assert hyperbench.schemas.QueryPack.__name__ == "QueryPack"
    assert hyperbench.schemas.CorpusManifest.__name__ == "CorpusManifest"
