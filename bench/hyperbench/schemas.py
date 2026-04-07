"""Typed schema contracts for the Phase 1 Hyperbench harness."""

from __future__ import annotations

from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Annotated, Literal, Self

import yaml
from pydantic import (
    BaseModel,
    ConfigDict,
    Field,
    HttpUrl,
    NonNegativeFloat,
    PositiveInt,
    model_validator,
)

SCHEMA_VERSION = "1"
SLUG_PATTERN = r"^[a-z0-9][a-z0-9-]*$"


class HyperbenchModel(BaseModel):
    """Base model with strict validation and JSON/YAML helpers."""

    model_config = ConfigDict(extra="forbid")

    @classmethod
    def from_json_text(cls, text: str) -> Self:
        return cls.model_validate_json(text)

    @classmethod
    def from_yaml_text(cls, text: str) -> Self:
        data = yaml.safe_load(text)
        return cls.model_validate(data)

    @classmethod
    def from_path(cls, path: str | Path) -> Self:
        path_obj = Path(path)
        text = path_obj.read_text(encoding="utf-8")
        suffix = path_obj.suffix.lower()
        if suffix == ".json":
            return cls.from_json_text(text)
        if suffix in {".yaml", ".yml"}:
            return cls.from_yaml_text(text)
        raise ValueError(
            f"Unsupported config format for {path_obj}; expected .json, .yaml, or .yml"
        )

    def to_json_text(self) -> str:
        return self.model_dump_json(indent=2)

    def to_yaml_text(self) -> str:
        return yaml.safe_dump(self.model_dump(mode="json"), sort_keys=False)


class RepoTier(str, Enum):
    S = "S"
    M = "M"
    L = "L"
    XL = "XL"


class CorpusKind(str, Enum):
    SYNTHETIC = "synthetic"
    EXTERNAL = "external"


class SelectionStatus(str, Enum):
    SELECTED = "selected"
    PLACEHOLDER = "placeholder"


class VerificationStatus(str, Enum):
    VERIFIED = "verified"
    PARTIAL = "partial"
    UNKNOWN = "unknown"


class HardwareClass(str, Enum):
    PRIMARY_LAPTOP = "primary_laptop"
    SECONDARY_DESKTOP = "secondary_desktop"
    STRETCH_FLOOR = "stretch_floor"


class OperatingSystem(str, Enum):
    MACOS = "macos"
    LINUX = "linux"


class Architecture(str, Enum):
    ARM64 = "arm64"
    X86_64 = "x86_64"


class StorageKind(str, Enum):
    SSD = "ssd"
    NVME = "nvme"


class CloneStrategy(str, Enum):
    SHALLOW = "shallow"
    FILTER_BLOBLESS = "filter_blobless"
    FULL = "full"


class PackageRole(str, Enum):
    AUTH = "auth"
    SESSION = "session"
    API = "api"
    WORKER = "worker"
    TESTS = "tests"
    WEB = "web"
    DATA = "data"


class QueryType(str, Enum):
    EXACT = "exact"
    SYMBOL = "symbol"
    SEMANTIC = "semantic"
    IMPACT = "impact"


class QueryLanguage(str, Enum):
    TYPESCRIPT = "typescript"
    TSX = "tsx"
    JAVASCRIPT = "javascript"
    JSX = "jsx"


class ExactMode(str, Enum):
    PLAIN = "plain"
    REGEX = "regex"


class SymbolScope(str, Enum):
    REPO = "repo"
    PACKAGE = "package"
    FILE = "file"


class SemanticRerankMode(str, Enum):
    OFF = "off"
    HYBRID = "hybrid"


class ImpactTargetType(str, Enum):
    SYMBOL = "symbol"
    FILE = "file"
    ROUTE = "route"
    CONFIG_KEY = "config_key"


class ChangeHint(str, Enum):
    MODIFY_BEHAVIOR = "modify_behavior"
    RENAME = "rename"
    SIGNATURE_CHANGE = "signature_change"
    DELETE = "delete"


class GoldenReason(str, Enum):
    TEXT = "text"
    DEFINITION = "definition"
    REFERENCE = "reference"
    CALLER = "caller"
    CALLEE = "callee"
    ROUTE = "route"
    CONFIG = "config"
    TEST = "test"
    SEMANTIC = "semantic"


class MetricUnit(str, Enum):
    MILLISECONDS = "ms"
    COUNT = "count"
    RATIO = "ratio"
    PERCENT = "percent"
    BYTES = "bytes"


class MetricKind(str, Enum):
    LATENCY = "latency"
    ACCURACY = "accuracy"
    SYSTEM = "system"
    CUSTOM = "custom"


class BudgetSeverity(str, Enum):
    WARN = "warn"
    FAIL = "fail"


class CompareVerdict(str, Enum):
    PASS = "pass"
    WARN = "warn"
    FAIL = "fail"


class BudgetStatus(str, Enum):
    PASS = "pass"
    WARN = "warn"
    FAIL = "fail"


class IntegerRange(HyperbenchModel):
    minimum: PositiveInt
    maximum: PositiveInt | None = None

    @model_validator(mode="after")
    def validate_bounds(self) -> Self:
        if self.maximum is not None and self.maximum < self.minimum:
            raise ValueError("maximum must be greater than or equal to minimum")
        return self


class RepoTierSpec(HyperbenchModel):
    tier: RepoTier
    loc_range: IntegerRange
    package_range: IntegerRange


class RepoTierCatalog(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    tiers: list[RepoTierSpec] = Field(min_length=1)

    @model_validator(mode="after")
    def validate_unique_tiers(self) -> Self:
        seen = [spec.tier for spec in self.tiers]
        if len(seen) != len(set(seen)):
            raise ValueError("tiers must not contain duplicate tier entries")
        return self


class RepoLicense(HyperbenchModel):
    spdx_id: str | None = Field(default=None, min_length=1)
    verification_status: VerificationStatus
    source_url: HttpUrl | None = None
    notes: str | None = None

    @model_validator(mode="after")
    def validate_unknown_license_notes(self) -> Self:
        if self.spdx_id is None and self.verification_status == VerificationStatus.VERIFIED:
            raise ValueError("spdx_id is required when license verification_status is verified")
        if self.verification_status != VerificationStatus.VERIFIED and not self.notes:
            raise ValueError(
                "license notes are required when license verification_status is partial or unknown"
            )
        return self


class RealRepoSelection(HyperbenchModel):
    repo_id: str = Field(pattern=SLUG_PATTERN)
    status: SelectionStatus
    owner: str = Field(min_length=1)
    name: str = Field(min_length=1)
    repo_url: HttpUrl
    clone_url: HttpUrl
    rationale: str = Field(min_length=1)
    expected_tier: RepoTier
    tier_verification_status: VerificationStatus = VerificationStatus.PARTIAL
    why_useful: str = Field(min_length=1)
    license: RepoLicense
    clone_strategy: CloneStrategy
    pinned_ref: str | None = Field(default=None, min_length=1)
    pinning_policy: str = Field(min_length=1)
    source_urls: list[HttpUrl] = Field(min_length=1)
    risks: list[str] = Field(min_length=1)
    manual_verification: list[str] = Field(default_factory=list)

    @model_validator(mode="after")
    def validate_manual_follow_up(self) -> Self:
        needs_follow_up = (
            self.status == SelectionStatus.PLACEHOLDER
            or self.tier_verification_status != VerificationStatus.VERIFIED
            or self.license.verification_status != VerificationStatus.VERIFIED
        )
        if needs_follow_up and not self.manual_verification:
            raise ValueError(
                "manual_verification is required for placeholder or partially verified repo entries"
            )
        return self


class RealRepoCatalog(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    generated_for_phase: Literal["phase1"] = "phase1"
    selection_note: str = Field(min_length=1)
    repos: list[RealRepoSelection] = Field(min_length=3)

    @model_validator(mode="after")
    def validate_repo_selection_shape(self) -> Self:
        repo_ids = [repo.repo_id for repo in self.repos]
        if len(repo_ids) != len(set(repo_ids)):
            raise ValueError("repos must not contain duplicate repo_id values")
        required_tiers = {RepoTier.S, RepoTier.M, RepoTier.L}
        present_tiers = {repo.expected_tier for repo in self.repos}
        missing_tiers = required_tiers - present_tiers
        if missing_tiers:
            missing = ", ".join(sorted(tier.value for tier in missing_tiers))
            raise ValueError(
                "repos must include at least one entry for each required tier: "
                f"{missing}"
            )
        return self


class CorpusSnapshot(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    source_path: str = Field(min_length=1)
    manifest_hash: str = Field(pattern=r"^[0-9a-f]{64}$")
    recorded_at: datetime
    commit_sha: str | None = Field(default=None, pattern=r"^[0-9a-f]{7,40}$")
    file_count: int = Field(ge=0)
    loc: int = Field(ge=0)
    package_count: int | None = Field(default=None, ge=0)
    notes: list[str] = Field(default_factory=list)
    warnings: list[str] = Field(default_factory=list)


class BenchmarkHardwareTarget(HyperbenchModel):
    target_id: str = Field(pattern=SLUG_PATTERN)
    name: str = Field(min_length=1)
    target_class: HardwareClass
    os_family: OperatingSystem
    architecture: Architecture
    cpu_cores_min: PositiveInt
    ram_gb: PositiveInt
    storage_kind: StorageKind
    notes: str | None = None


class BenchmarkHardwareCatalog(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    hardware_targets: list[BenchmarkHardwareTarget] = Field(min_length=1)

    @model_validator(mode="after")
    def validate_unique_target_ids(self) -> Self:
        target_ids = [target.target_id for target in self.hardware_targets]
        if len(target_ids) != len(set(target_ids)):
            raise ValueError("hardware_targets must not contain duplicate target_id values")
        return self


class SyntheticCorpusConfig(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    config_id: str = Field(pattern=SLUG_PATTERN)
    corpus_id: str = Field(pattern=SLUG_PATTERN)
    repo_tier: RepoTier
    seed: int = Field(ge=0)
    scenario: Literal["saas_monorepo"] = "saas_monorepo"
    package_manager: Literal["pnpm"] = "pnpm"
    package_count: PositiveInt
    file_count: PositiveInt = 64
    dependency_fanout: int = Field(default=2, ge=0)
    route_count: PositiveInt = 4
    handler_count: PositiveInt = 4
    config_file_count: PositiveInt = 3
    test_file_count: PositiveInt = 4
    auth_flow_count: PositiveInt = 3
    exports_per_package: PositiveInt = 2
    edit_scenario_count: PositiveInt = 3
    query_seed_count: PositiveInt = 6
    required_package_roles: list[PackageRole] = Field(min_length=1)
    include_session_invalidation_path: bool = True
    workspace_package_prefix: str = Field(min_length=1)
    notes: str | None = None

    @model_validator(mode="after")
    def validate_roles(self) -> Self:
        required_core = {
            PackageRole.AUTH,
            PackageRole.SESSION,
            PackageRole.API,
            PackageRole.WORKER,
            PackageRole.TESTS,
        }
        provided = set(self.required_package_roles)
        missing = required_core - provided
        if missing:
            missing_roles = ", ".join(sorted(role.value for role in missing))
            raise ValueError(
                "required_package_roles must include the core scenario roles: "
                f"{missing_roles}"
            )
        if self.package_count < len(provided):
            raise ValueError("package_count must be at least the number of required_package_roles")
        if self.include_session_invalidation_path:
            if self.route_count < 1:
                raise ValueError(
                    "route_count must be at least 1 when hero-path coverage is enabled"
                )
            if self.handler_count < 1:
                raise ValueError(
                    "handler_count must be at least 1 when hero-path coverage is enabled"
                )
            if self.config_file_count < 1:
                raise ValueError(
                    "config_file_count must be at least 1 when hero-path coverage is enabled"
                )
            if self.test_file_count < 1:
                raise ValueError(
                    "test_file_count must be at least 1 when hero-path coverage is enabled"
                )
            if self.auth_flow_count < 1:
                raise ValueError(
                    "auth_flow_count must be at least 1 when hero-path coverage is enabled"
                )
        return self


class CorpusManifest(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    corpus_id: str = Field(pattern=SLUG_PATTERN)
    display_name: str = Field(min_length=1)
    kind: CorpusKind
    language: Literal["typescript"] = "typescript"
    repo_tier: RepoTier
    description: str = Field(min_length=1)
    deterministic: bool = True
    source_uri: str | None = None
    local_path: str | None = None
    synthetic_config_id: str | None = Field(default=None, pattern=SLUG_PATTERN)
    bootstrap_commands: list[str] = Field(default_factory=list)
    query_pack_ids: list[str] = Field(default_factory=list)
    golden_set_ids: list[str] = Field(default_factory=list)
    hardware_target_ids: list[str] = Field(default_factory=list)

    @model_validator(mode="after")
    def validate_manifest_shape(self) -> Self:
        if self.kind == CorpusKind.SYNTHETIC and not self.synthetic_config_id:
            raise ValueError("synthetic corpora must set synthetic_config_id")
        if self.kind == CorpusKind.EXTERNAL and not (self.source_uri or self.local_path):
            raise ValueError("external corpora must set source_uri or local_path")
        return self


class QueryBase(HyperbenchModel):
    query_id: str = Field(pattern=SLUG_PATTERN)
    title: str = Field(min_length=1)
    tags: list[str] = Field(default_factory=list)
    notes: str | None = Field(default=None, min_length=1)
    limit: PositiveInt = Field(default=20, le=100)

    @model_validator(mode="after")
    def validate_unique_tags(self) -> Self:
        if len(self.tags) != len(set(self.tags)):
            raise ValueError("query tags must not contain duplicates")
        return self


class ExactQuery(QueryBase):
    type: Literal["exact"] = QueryType.EXACT.value
    text: str = Field(min_length=1)
    mode: ExactMode = ExactMode.PLAIN
    path_globs: list[str] = Field(default_factory=list)
    languages: list[QueryLanguage] = Field(
        default_factory=lambda: [QueryLanguage.TYPESCRIPT, QueryLanguage.TSX]
    )


class SymbolQuery(QueryBase):
    type: Literal["symbol"] = QueryType.SYMBOL.value
    symbol: str = Field(min_length=1)
    scope: SymbolScope = SymbolScope.REPO


class SemanticQuery(QueryBase):
    type: Literal["semantic"] = QueryType.SEMANTIC.value
    text: str = Field(min_length=1)
    path_globs: list[str] = Field(default_factory=list)
    rerank_mode: SemanticRerankMode = SemanticRerankMode.HYBRID


class ImpactQuery(QueryBase):
    type: Literal["impact"] = QueryType.IMPACT.value
    target_type: ImpactTargetType
    target: str = Field(min_length=1)
    change_hint: ChangeHint = ChangeHint.MODIFY_BEHAVIOR


QueryModel = Annotated[
    ExactQuery | SymbolQuery | SemanticQuery | ImpactQuery,
    Field(discriminator="type"),
]


class QueryPack(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    query_pack_id: str = Field(pattern=SLUG_PATTERN)
    corpus_id: str = Field(pattern=SLUG_PATTERN)
    description: str | None = None
    queries: list[QueryModel] = Field(min_length=1)

    @model_validator(mode="after")
    def validate_unique_query_ids(self) -> Self:
        query_ids = [query.query_id for query in self.queries]
        if len(query_ids) != len(set(query_ids)):
            raise ValueError("queries must not contain duplicate query_id values")
        return self


class ExpectedHit(HyperbenchModel):
    path: str = Field(min_length=1)
    symbol: str | None = None
    reason: GoldenReason
    rank_max: PositiveInt = 5
    min_score: float | None = Field(default=None, ge=0.0, le=1.0)


class GoldenExpectation(HyperbenchModel):
    query_id: str = Field(pattern=SLUG_PATTERN)
    expected_hits: list[ExpectedHit] = Field(min_length=1)
    expected_top_hit: ExpectedHit | None = None
    max_latency_ms: PositiveInt | None = None
    notes: str | None = None


class GoldenSet(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    golden_set_id: str = Field(pattern=SLUG_PATTERN)
    corpus_id: str = Field(pattern=SLUG_PATTERN)
    query_pack_id: str = Field(pattern=SLUG_PATTERN)
    expectations: list[GoldenExpectation] = Field(min_length=1)

    @model_validator(mode="after")
    def validate_unique_expectation_queries(self) -> Self:
        query_ids = [expectation.query_id for expectation in self.expectations]
        if len(query_ids) != len(set(query_ids)):
            raise ValueError("expectations must not contain duplicate query_id values")
        return self


class RunMetadata(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    run_id: str = Field(pattern=SLUG_PATTERN)
    recorded_at: datetime
    harness_version: str = Field(min_length=1)
    engine_name: str = Field(min_length=1)
    engine_version: str | None = None
    corpus_id: str = Field(pattern=SLUG_PATTERN)
    query_pack_id: str = Field(pattern=SLUG_PATTERN)
    hardware_target_id: str = Field(pattern=SLUG_PATTERN)
    git_commit: str | None = Field(default=None, pattern=r"^[0-9a-f]{7,40}$")
    command: list[str] = Field(default_factory=list)
    notes: str | None = None


class MetricSample(HyperbenchModel):
    metric_name: str = Field(pattern=SLUG_PATTERN)
    metric_kind: MetricKind
    unit: MetricUnit
    value: NonNegativeFloat
    tags: dict[str, str] = Field(default_factory=dict)


class MetricSummary(HyperbenchModel):
    metric_name: str = Field(pattern=SLUG_PATTERN)
    metric_kind: MetricKind
    unit: MetricUnit
    sample_count: PositiveInt
    minimum: NonNegativeFloat
    maximum: NonNegativeFloat
    mean: NonNegativeFloat
    p50: NonNegativeFloat | None = None
    p95: NonNegativeFloat | None = None
    p99: NonNegativeFloat | None = None

    @model_validator(mode="after")
    def validate_summary_order(self) -> Self:
        if self.minimum > self.maximum:
            raise ValueError("minimum must be less than or equal to maximum")
        if not (self.minimum <= self.mean <= self.maximum):
            raise ValueError("mean must be between minimum and maximum")
        for percentile_name in ("p50", "p95", "p99"):
            percentile_value = getattr(self, percentile_name)
            if percentile_value is not None and not (
                self.minimum <= percentile_value <= self.maximum
            ):
                raise ValueError(f"{percentile_name} must be between minimum and maximum")
        return self


class MetricsDocument(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    run_id: str = Field(pattern=SLUG_PATTERN)
    samples: list[MetricSample] = Field(default_factory=list)
    summaries: list[MetricSummary] = Field(default_factory=list)

    @model_validator(mode="after")
    def validate_non_empty_document(self) -> Self:
        if not self.samples and not self.summaries:
            raise ValueError("metrics documents must contain at least one sample or summary")
        return self


class BudgetThreshold(HyperbenchModel):
    metric_name: str = Field(pattern=SLUG_PATTERN)
    unit: MetricUnit
    max_value: NonNegativeFloat | None = None
    min_value: NonNegativeFloat | None = None
    max_regression_pct: NonNegativeFloat | None = None
    severity: BudgetSeverity = BudgetSeverity.FAIL

    @model_validator(mode="after")
    def validate_threshold(self) -> Self:
        if (
            self.max_value is None
            and self.min_value is None
            and self.max_regression_pct is None
        ):
            raise ValueError("budget thresholds must define at least one limit")
        return self


class CompareBudget(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    budget_id: str = Field(pattern=SLUG_PATTERN)
    thresholds: list[BudgetThreshold] = Field(min_length=1)

    @model_validator(mode="after")
    def validate_unique_budget_metrics(self) -> Self:
        metric_names = [threshold.metric_name for threshold in self.thresholds]
        if len(metric_names) != len(set(metric_names)):
            raise ValueError("thresholds must not contain duplicate metric_name values")
        return self


class MetricDelta(HyperbenchModel):
    metric_name: str = Field(pattern=SLUG_PATTERN)
    unit: MetricUnit
    baseline_value: NonNegativeFloat
    candidate_value: NonNegativeFloat
    absolute_delta: float
    percent_delta: float | None = None


class BudgetResult(HyperbenchModel):
    metric_name: str = Field(pattern=SLUG_PATTERN)
    status: BudgetStatus
    message: str = Field(min_length=1)
    observed_value: NonNegativeFloat | None = None


class CompareOutput(HyperbenchModel):
    schema_version: Literal["1"] = SCHEMA_VERSION
    baseline_run_id: str = Field(pattern=SLUG_PATTERN)
    candidate_run_id: str = Field(pattern=SLUG_PATTERN)
    verdict: CompareVerdict
    metric_deltas: list[MetricDelta] = Field(default_factory=list)
    budget_results: list[BudgetResult] = Field(default_factory=list)

    @model_validator(mode="after")
    def validate_runs(self) -> Self:
        if self.baseline_run_id == self.candidate_run_id:
            raise ValueError("baseline_run_id and candidate_run_id must differ")
        return self


__all__ = [
    "Architecture",
    "BenchmarkHardwareCatalog",
    "BenchmarkHardwareTarget",
    "BudgetResult",
    "BudgetSeverity",
    "BudgetStatus",
    "BudgetThreshold",
    "ChangeHint",
    "CompareBudget",
    "CompareOutput",
    "CompareVerdict",
    "CorpusKind",
    "CorpusManifest",
    "CorpusSnapshot",
    "ExactMode",
    "ExactQuery",
    "ExpectedHit",
    "GoldenExpectation",
    "GoldenReason",
    "GoldenSet",
    "HardwareClass",
    "HyperbenchModel",
    "ImpactQuery",
    "ImpactTargetType",
    "IntegerRange",
    "MetricDelta",
    "MetricKind",
    "MetricSample",
    "MetricSummary",
    "MetricUnit",
    "MetricsDocument",
    "OperatingSystem",
    "PackageRole",
    "QueryLanguage",
    "QueryModel",
    "QueryPack",
    "QueryType",
    "RealRepoCatalog",
    "RealRepoSelection",
    "RepoTier",
    "RepoTierCatalog",
    "RepoTierSpec",
    "RunMetadata",
    "SCHEMA_VERSION",
    "SemanticQuery",
    "SemanticRerankMode",
    "StorageKind",
    "SymbolQuery",
    "SymbolScope",
    "SyntheticCorpusConfig",
]
