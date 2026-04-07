"""Deterministic synthetic TypeScript monorepo generator for Hyperbench."""

from __future__ import annotations

import json
import shutil
from dataclasses import dataclass, field
from pathlib import Path

from hyperbench.schemas import (
    ChangeHint,
    CorpusKind,
    CorpusManifest,
    ExactMode,
    ExactQuery,
    ExpectedHit,
    GoldenExpectation,
    GoldenReason,
    GoldenSet,
    ImpactQuery,
    ImpactTargetType,
    QueryPack,
    QueryType,
    SemanticQuery,
    SemanticRerankMode,
    SymbolQuery,
    SymbolScope,
    SyntheticCorpusConfig,
)

ROOT_FILES = (
    "package.json",
    "pnpm-workspace.yaml",
    "tsconfig.base.json",
    "README.md",
)
EXTRA_PACKAGE_CANDIDATES = [
    "billing",
    "analytics",
    "notifications",
    "profiles",
    "search",
    "reporting",
    "admin",
    "shared",
    "support",
    "identity",
]
FLOW_CANDIDATES = [
    "logout",
    "password-reset",
    "admin-revoke",
    "device-revoke",
    "sso-logout",
    "mfa-step-up",
    "support-logout",
    "api-token-rotate",
]
CONFIG_CANDIDATES = [
    "auth-policy.ts",
    "route-registry.ts",
    "session-limits.json",
    "worker-flags.ts",
    "feature-flags.json",
    "test-matrix.ts",
]


class SyntheticGenerationError(RuntimeError):
    """Raised when synthetic corpus generation cannot proceed."""


@dataclass(frozen=True)
class SyntheticBundleResult:
    """Summary of a generated synthetic corpus bundle."""

    output_dir: Path
    repo_dir: Path
    manifest_path: Path
    ground_truth_path: Path
    query_pack_path: Path
    golden_set_path: Path
    query_pack_dir: Path
    golden_set_dir: Path
    edit_scenarios_path: Path
    repo_file_count: int


@dataclass
class PackagePlan:
    """Planned package details for the synthetic monorepo."""

    slug: str
    dependencies: list[str] = field(default_factory=list)
    exports: list[str] = field(default_factory=list)

    @property
    def package_name(self) -> str:
        return self.slug


@dataclass(frozen=True)
class GeneratedModuleSpec:
    """Metadata for a generated filler module used in query derivation."""

    package_slug: str
    path: str
    function_name: str
    dependency_constants: list[str]
    dependency_packages: list[str]


@dataclass(frozen=True)
class QueryArtifactPair:
    """A query and its matching typed golden expectation."""

    query: ExactQuery | SymbolQuery | SemanticQuery | ImpactQuery
    expectation: GoldenExpectation


def generate_synthetic_corpus_bundle(
    config: SyntheticCorpusConfig,
    output_dir: Path | str,
    *,
    force: bool = False,
) -> SyntheticBundleResult:
    """Generate a deterministic synthetic corpus bundle from a config."""
    bundle_dir = Path(output_dir)
    if bundle_dir.exists():
        if not force and any(bundle_dir.iterdir()):
            raise SyntheticGenerationError(
                f"Output directory already exists and is not empty: {bundle_dir}. "
                "Re-run with --force to replace it."
            )
        if force:
            shutil.rmtree(bundle_dir)

    repo_dir = bundle_dir / "repo"
    package_plans = _build_package_plans(config)
    flows = _build_flow_names(config)
    route_names = _extend_names(flows, "route", config.route_count)
    handler_names = _extend_names(flows, "handler", config.handler_count)
    test_names = _extend_names(flows, "session", config.test_file_count)
    config_names = _extend_names(CONFIG_CANDIDATES, "config", config.config_file_count)

    repo_files = _build_repo_files(
        config=config,
        package_plans=package_plans,
        route_names=route_names,
        handler_names=handler_names,
        test_names=test_names,
        config_names=config_names,
    )

    if len(repo_files) > config.file_count:
        raise SyntheticGenerationError(
            f"Configured file_count={config.file_count} is too small for the minimum "
            f"deterministic bundle shape ({len(repo_files)} repo files). Increase "
            "file_count or reduce category counts."
        )

    filler_count = config.file_count - len(repo_files)
    generated_modules = _add_filler_modules(
        repo_files=repo_files,
        config=config,
        package_plans=package_plans,
        filler_count=filler_count,
    )
    _finalize_package_indexes(repo_files, config, package_plans)

    query_packs, golden_sets = _build_generated_query_artifacts(
        config=config,
        package_plans=package_plans,
        route_names=route_names,
        handler_names=handler_names,
        test_names=test_names,
        config_names=config_names,
        generated_modules=generated_modules,
    )
    manifest = _build_generated_manifest(config, query_packs, golden_sets)
    query_pack = _combine_query_packs(config, query_packs)
    golden_set = _combine_golden_sets(config, query_pack.query_pack_id, golden_sets)
    ground_truth = _build_ground_truth(
        config=config,
        route_names=route_names,
        handler_names=handler_names,
        test_names=test_names,
        config_names=config_names,
        query_packs=query_packs,
        golden_sets=golden_sets,
    )
    edit_scenarios = _build_edit_scenarios(config, route_names)

    _write_repo_files(repo_dir, repo_files)
    _write_json(bundle_dir / "corpus-manifest.json", manifest.model_dump(mode="json"))
    _write_json(bundle_dir / "query-pack.json", query_pack.model_dump(mode="json"))
    _write_json(bundle_dir / "golden-set.json", golden_set.model_dump(mode="json"))
    _write_query_artifacts(bundle_dir / "query-packs", query_packs)
    _write_query_artifacts(bundle_dir / "goldens", golden_sets)
    _write_json(bundle_dir / "ground_truth.json", ground_truth)
    _write_json(bundle_dir / "edit_scenarios.json", edit_scenarios)

    return SyntheticBundleResult(
        output_dir=bundle_dir,
        repo_dir=repo_dir,
        manifest_path=bundle_dir / "corpus-manifest.json",
        ground_truth_path=bundle_dir / "ground_truth.json",
        query_pack_path=bundle_dir / "query-pack.json",
        golden_set_path=bundle_dir / "golden-set.json",
        query_pack_dir=bundle_dir / "query-packs",
        golden_set_dir=bundle_dir / "goldens",
        edit_scenarios_path=bundle_dir / "edit_scenarios.json",
        repo_file_count=len(repo_files),
    )


def _build_package_plans(config: SyntheticCorpusConfig) -> list[PackagePlan]:
    ordered_core = [role.value for role in config.required_package_roles]
    extras_needed = config.package_count - len(ordered_core)
    extras = _rotate_candidates(EXTRA_PACKAGE_CANDIDATES, config.seed, extras_needed)
    package_names = ordered_core + extras
    plans = [PackagePlan(slug=name) for name in package_names]

    for index, plan in enumerate(plans):
        previous = [candidate.slug for candidate in plans[:index]]
        selected = _rotated_slice(previous, config.seed + index, config.dependency_fanout)
        forced = _forced_dependencies(plan.slug, package_names)
        plan.dependencies = _unique_preserve(forced + selected)
    return plans


def _build_flow_names(config: SyntheticCorpusConfig) -> list[str]:
    base = _rotate_candidates(FLOW_CANDIDATES, config.seed, config.auth_flow_count)
    if "logout" not in base:
        return ["logout"] + base[:-1]
    if base[0] != "logout":
        return ["logout"] + [name for name in base if name != "logout"]
    return base


def _build_repo_files(
    *,
    config: SyntheticCorpusConfig,
    package_plans: list[PackagePlan],
    route_names: list[str],
    handler_names: list[str],
    test_names: list[str],
    config_names: list[str],
) -> dict[str, str]:
    repo_files: dict[str, str] = {}
    repo_files["package.json"] = _root_package_json(config, package_plans)
    repo_files["pnpm-workspace.yaml"] = "packages:\n  - packages/*\n"
    repo_files["tsconfig.base.json"] = _json_text(
        {
            "compilerOptions": {
                "target": "ES2022",
                "module": "ESNext",
                "moduleResolution": "Node",
                "strict": True,
                "baseUrl": ".",
            }
        }
    )
    repo_files["README.md"] = (
        f"# {config.corpus_id}\n\n"
        "Deterministic synthetic monorepo fixture for Repo Hyperindex Phase 1.\n"
    )

    for plan in package_plans:
        package_root = Path("packages") / plan.slug
        repo_files[str(package_root / "package.json")] = _package_json(config, plan)
        repo_files[str(package_root / "tsconfig.json")] = _json_text(
            {"extends": "../../tsconfig.base.json", "include": ["src/**/*.ts"]}
        )
        repo_files[str(package_root / "src" / "index.ts")] = ""

    repo_files["config/auth-policy.ts"] = (
        "export const authPolicy = {\n"
        "  invalidateOnPasswordReset: true,\n"
        "  invalidateOnRoleChange: true,\n"
        "  sessionTtlMinutes: 120,\n"
        "};\n"
    )
    for file_name in config_names[1:]:
        content = (
            f"export const { _safe_identifier(Path(file_name).stem) } = {{\n"
            f"  seed: {config.seed},\n"
            f"  enabled: true,\n"
            "};\n"
        )
        repo_files[f"config/{file_name}"] = content

    repo_files["packages/session/src/store/session-store.ts"] = _session_store_content()
    _register_export(package_plans, "session", "./src/store/session-store")

    repo_files["packages/auth/src/session/service.ts"] = _auth_service_content(config)
    _register_export(package_plans, "auth", "./src/session/service")

    repo_files["packages/worker/src/jobs/password-reset.ts"] = _worker_job_content()
    _register_export(package_plans, "worker", "./src/jobs/password-reset")

    repo_files["packages/api/src/handlers/logout-handler.ts"] = _handler_content("logout")
    _register_export(package_plans, "api", "./src/handlers/logout-handler")
    repo_files["packages/api/src/routes/logout.ts"] = _route_content("logout", "logout")
    _register_export(package_plans, "api", "./src/routes/logout")

    for handler_name in handler_names[1:]:
        handler_path = f"packages/api/src/handlers/{handler_name}-handler.ts"
        repo_files[handler_path] = _handler_content(handler_name)
        _register_export(package_plans, "api", f"./src/handlers/{handler_name}-handler")

    for index, route_name in enumerate(route_names[1:], start=1):
        handler_name = handler_names[min(index, len(handler_names) - 1)]
        route_path = f"packages/api/src/routes/{route_name}.ts"
        repo_files[route_path] = _route_content(route_name, handler_name)
        _register_export(package_plans, "api", f"./src/routes/{route_name}")

    repo_files["packages/tests/src/session/session.test.ts"] = _hero_test_content()
    for test_name in test_names[1:]:
        repo_files[f"packages/tests/src/session/{test_name}.test.ts"] = _generic_test_content(
            test_name
        )

    return repo_files


def _add_filler_modules(
    *,
    repo_files: dict[str, str],
    config: SyntheticCorpusConfig,
    package_plans: list[PackagePlan],
    filler_count: int,
) -> list[GeneratedModuleSpec]:
    generated_modules: list[GeneratedModuleSpec] = []
    if filler_count == 0:
        return generated_modules
    candidate_plans = [plan for plan in package_plans if plan.slug != "tests"]
    for index in range(filler_count):
        plan = candidate_plans[index % len(candidate_plans)]
        module_name = f"generated-{index + 1:03d}"
        rel_path = f"packages/{plan.slug}/src/generated/{module_name}.ts"
        repo_files[rel_path] = _generated_module_content(plan, index)
        _register_export(package_plans, plan.slug, f"./src/generated/{module_name}")
        generated_modules.append(
            GeneratedModuleSpec(
                package_slug=plan.slug,
                path=rel_path,
                function_name=f"{_safe_identifier(plan.slug)}Generated{index + 1:03d}",
                dependency_constants=[
                    f"{_constant_name(dep)}_PACKAGE_NAME" for dep in plan.dependencies
                ],
                dependency_packages=list(plan.dependencies),
            )
        )
    return generated_modules


def _finalize_package_indexes(
    repo_files: dict[str, str],
    config: SyntheticCorpusConfig,
    package_plans: list[PackagePlan],
) -> None:
    for plan in package_plans:
        constant_prefix = _constant_name(plan.slug)
        export_lines = [f'export const {constant_prefix}_PACKAGE_SLUG = "{plan.slug}";']
        export_lines.append(
            f'export const {constant_prefix}_PACKAGE_NAME = '
            f'"{config.workspace_package_prefix}/{plan.slug}";'
        )
        for export_path in sorted(set(plan.exports)):
            export_lines.append(f'export * from "{export_path}";')
        repo_files[f"packages/{plan.slug}/src/index.ts"] = "\n".join(export_lines) + "\n"


def _build_generated_manifest(
    config: SyntheticCorpusConfig,
    query_packs: list[QueryPack],
    golden_sets: list[GoldenSet],
) -> CorpusManifest:
    return CorpusManifest(
        corpus_id=config.corpus_id,
        display_name=f"Synthetic {config.repo_tier.value} {config.corpus_id}",
        kind=CorpusKind.SYNTHETIC,
        repo_tier=config.repo_tier,
        description=(
            "Deterministic synthetic TypeScript monorepo with auth/session hero-path coverage "
            "for Repo Hyperindex Phase 1."
        ),
        deterministic=True,
        local_path="repo",
        synthetic_config_id=config.config_id,
        query_pack_ids=[pack.query_pack_id for pack in query_packs],
        golden_set_ids=[golden.golden_set_id for golden in golden_sets],
        hardware_target_ids=[],
    )


def _build_generated_query_artifacts(
    *,
    config: SyntheticCorpusConfig,
    package_plans: list[PackagePlan],
    route_names: list[str],
    handler_names: list[str],
    test_names: list[str],
    config_names: list[str],
    generated_modules: list[GeneratedModuleSpec],
) -> tuple[list[QueryPack], list[GoldenSet]]:
    exact_pairs = _build_exact_query_pairs(
        config=config,
        package_plans=package_plans,
        route_names=route_names,
        handler_names=handler_names,
        generated_modules=generated_modules,
    )
    symbol_pairs = _build_symbol_query_pairs(
        config=config,
        package_plans=package_plans,
        route_names=route_names,
        handler_names=handler_names,
        config_names=config_names,
        generated_modules=generated_modules,
    )
    semantic_pairs = _build_semantic_query_pairs(
        config=config,
        route_names=route_names,
        handler_names=handler_names,
        test_names=test_names,
        config_names=config_names,
        generated_modules=generated_modules,
    )
    impact_pairs = _build_impact_query_pairs(
        config=config,
        route_names=route_names,
        generated_modules=generated_modules,
    )

    pair_sets = {
        QueryType.EXACT: exact_pairs,
        QueryType.SYMBOL: symbol_pairs,
        QueryType.SEMANTIC: semantic_pairs,
        QueryType.IMPACT: impact_pairs,
    }
    query_packs: list[QueryPack] = []
    golden_sets: list[GoldenSet] = []
    for query_type, pairs in pair_sets.items():
        query_packs.append(_pairs_to_query_pack(config, query_type, pairs))
        golden_sets.append(_pairs_to_golden_set(config, query_type, pairs))
    return query_packs, golden_sets


def _build_exact_query_pairs(
    *,
    config: SyntheticCorpusConfig,
    package_plans: list[PackagePlan],
    route_names: list[str],
    handler_names: list[str],
    generated_modules: list[GeneratedModuleSpec],
) -> list[QueryArtifactPair]:
    pairs: list[QueryArtifactPair] = []
    seen: set[tuple[str, str]] = set()
    hero_literals = [
        (
            "exact-invalidate-session",
            "Find invalidateSession by exact text",
            "invalidateSession",
            "packages/auth/src/session/service.ts",
            "invalidateSession",
            ["synthetic", "exact", "hero", "session", "auth"],
        ),
        (
            "exact-revoke-all-user-sessions",
            "Find revokeAllUserSessions by exact text",
            "revokeAllUserSessions",
            "packages/auth/src/session/service.ts",
            "revokeAllUserSessions",
            ["synthetic", "exact", "auth", "session-bulk"],
        ),
        (
            "exact-session-store",
            "Find sessionStore by exact text",
            "sessionStore",
            "packages/session/src/store/session-store.ts",
            "sessionStore",
            ["synthetic", "exact", "session", "storage"],
        ),
        (
            "exact-record-session-event",
            "Find recordSessionEvent by exact text",
            "recordSessionEvent",
            "packages/session/src/store/session-store.ts",
            "recordSessionEvent",
            ["synthetic", "exact", "session", "events"],
        ),
        (
            "exact-invalidate-result",
            "Find InvalidateResult by exact text",
            "InvalidateResult",
            "packages/auth/src/session/service.ts",
            "InvalidateResult",
            ["synthetic", "exact", "auth", "types"],
        ),
        (
            "exact-auth-policy",
            "Find authPolicy by exact text",
            "authPolicy",
            "config/auth-policy.ts",
            "authPolicy",
            ["synthetic", "exact", "config", "auth"],
        ),
        (
            "exact-logout-route",
            "Find logoutRoute by exact text",
            "logoutRoute",
            "packages/api/src/routes/logout.ts",
            "logoutRoute",
            ["synthetic", "exact", "hero", "route"],
        ),
        (
            "exact-logout-handler",
            "Find logoutHandler by exact text",
            "logoutHandler",
            "packages/api/src/handlers/logout-handler.ts",
            "logoutHandler",
            ["synthetic", "exact", "hero", "handler"],
        ),
        (
            "exact-handle-password-reset",
            "Find handlePasswordReset by exact text",
            "handlePasswordReset",
            "packages/worker/src/jobs/password-reset.ts",
            "handlePasswordReset",
            ["synthetic", "exact", "worker", "password-reset"],
        ),
        (
            "exact-password-reset-flag",
            "Find invalidateOnPasswordReset by exact text",
            "invalidateOnPasswordReset",
            "config/auth-policy.ts",
            None,
            ["synthetic", "exact", "config", "hero"],
        ),
        (
            "exact-role-change-flag",
            "Find invalidateOnRoleChange by exact text",
            "invalidateOnRoleChange",
            "config/auth-policy.ts",
            None,
            ["synthetic", "exact", "config"],
        ),
        (
            "exact-session-ttl",
            "Find sessionTtlMinutes by exact text",
            "sessionTtlMinutes",
            "config/auth-policy.ts",
            None,
            ["synthetic", "exact", "config", "session"],
        ),
        (
            "exact-session-invalidated-event",
            "Find session.invalidated by exact text",
            "session.invalidated",
            "packages/auth/src/session/service.ts",
            None,
            ["synthetic", "exact", "events", "hero"],
        ),
        (
            "exact-session-bulk-invalidated-event",
            "Find session.bulk-invalidated by exact text",
            "session.bulk-invalidated",
            "packages/auth/src/session/service.ts",
            None,
            ["synthetic", "exact", "events"],
        ),
        (
            "exact-password-reset-worker-tag",
            "Find password-reset-worker by exact text",
            "password-reset-worker",
            "packages/worker/src/jobs/password-reset.ts",
            None,
            ["synthetic", "exact", "worker", "events"],
        ),
    ]
    for query_id, title, text, path, symbol, tags in hero_literals:
        _append_exact_pair(
            pairs,
            seen,
            query_id=query_id,
            title=title,
            text=text,
            path=path,
            symbol=symbol,
            tags=tags,
        )

    for plan in package_plans:
        package_json_path = f"packages/{plan.slug}/package.json"
        package_name = f"{config.workspace_package_prefix}/{plan.slug}"
        _append_exact_pair(
            pairs,
            seen,
            query_id=f"exact-package-name-{plan.slug}",
            title=f"Find the {plan.slug} package name string",
            text=package_name,
            path=package_json_path,
            tags=["synthetic", "exact", "package", plan.slug],
        )

    for pair in _build_symbol_query_pairs(
        config=config,
        package_plans=package_plans,
        route_names=route_names,
        handler_names=handler_names,
        config_names=["auth-policy.ts"],
        generated_modules=generated_modules,
    ):
        expectation = pair.expectation.expected_hits[0]
        _append_exact_pair(
            pairs,
            seen,
            query_id=f"exact-{pair.query.query_id.removeprefix('symbol-')}",
            title=f"Find {pair.query.symbol} by exact text",
            text=pair.query.symbol,
            path=expectation.path,
            symbol=expectation.symbol,
            tags=["synthetic", "exact", *pair.query.tags],
        )

    for module in generated_modules:
        if module.dependency_constants:
            _append_exact_pair(
                pairs,
                seen,
                query_id=f"exact-{module.package_slug}-{_module_suffix(module.path)}-dep-const",
                title=(
                    f"Find the dependency constant in "
                    f"{module.package_slug}/{Path(module.path).stem}"
                ),
                text=module.dependency_constants[0],
                path=module.path,
                tags=["synthetic", "exact", "generated", module.package_slug],
            )
        if module.dependency_packages:
            _append_exact_pair(
                pairs,
                seen,
                query_id=f"exact-{module.package_slug}-{_module_suffix(module.path)}-dep-import",
                title=(
                    f"Find the dependency import in "
                    f"{module.package_slug}/{Path(module.path).stem}"
                ),
                text=f"{config.workspace_package_prefix}/{module.dependency_packages[0]}",
                path=module.path,
                tags=["synthetic", "exact", "generated", "imports", module.package_slug],
            )

    return pairs[:100]


def _build_symbol_query_pairs(
    *,
    config: SyntheticCorpusConfig,
    package_plans: list[PackagePlan],
    route_names: list[str],
    handler_names: list[str],
    config_names: list[str],
    generated_modules: list[GeneratedModuleSpec],
) -> list[QueryArtifactPair]:
    pairs: list[QueryArtifactPair] = []
    seen_ids: set[str] = set()
    route_labels = _unique_preserve(["logout", *route_names[1:]])
    handler_labels = _unique_preserve(["logout", *handler_names[1:]])
    fixed_symbols = [
        (
            "session-store",
            "Resolve sessionStore",
            "sessionStore",
            "packages/session/src/store/session-store.ts",
            ["synthetic", "symbol", "session"],
        ),
        (
            "record-session-event",
            "Resolve recordSessionEvent",
            "recordSessionEvent",
            "packages/session/src/store/session-store.ts",
            ["synthetic", "symbol", "session", "events"],
        ),
        (
            "invalidate-result",
            "Resolve InvalidateResult",
            "InvalidateResult",
            "packages/auth/src/session/service.ts",
            ["synthetic", "symbol", "auth", "types"],
        ),
        (
            "invalidate-session",
            "Resolve invalidateSession",
            "invalidateSession",
            "packages/auth/src/session/service.ts",
            ["synthetic", "symbol", "hero", "session"],
        ),
        (
            "revoke-all-user-sessions",
            "Resolve revokeAllUserSessions",
            "revokeAllUserSessions",
            "packages/auth/src/session/service.ts",
            ["synthetic", "symbol", "auth", "session-bulk"],
        ),
        (
            "auth-policy",
            "Resolve authPolicy",
            "authPolicy",
            "config/auth-policy.ts",
            ["synthetic", "symbol", "config", "auth"],
        ),
        (
            "handle-password-reset",
            "Resolve handlePasswordReset",
            "handlePasswordReset",
            "packages/worker/src/jobs/password-reset.ts",
            ["synthetic", "symbol", "worker", "password-reset"],
        ),
    ]
    for slug, title, symbol, path, tags in fixed_symbols:
        _append_symbol_pair(pairs, seen_ids, slug, title, symbol, path, tags)

    for handler_name in handler_labels:
        function_name = f"{_camel_case(handler_name)}Handler"
        _append_symbol_pair(
            pairs,
            seen_ids,
            f"{handler_name}-handler",
            f"Resolve {function_name}",
            function_name,
            f"packages/api/src/handlers/{handler_name}-handler.ts",
            ["synthetic", "symbol", "handler", handler_name],
        )

    for route_name in route_labels:
        function_name = f"{_camel_case(route_name)}Route"
        _append_symbol_pair(
            pairs,
            seen_ids,
            f"{route_name}-route",
            f"Resolve {function_name}",
            function_name,
            f"packages/api/src/routes/{route_name}.ts",
            ["synthetic", "symbol", "route", route_name],
        )

    for file_name in config_names[1:]:
        export_name = _safe_identifier(Path(file_name).stem)
        _append_symbol_pair(
            pairs,
            seen_ids,
            f"config-{_slugify(Path(file_name).stem)}",
            f"Resolve {export_name}",
            export_name,
            f"config/{file_name}",
            ["synthetic", "symbol", "config"],
        )

    for plan in package_plans:
        slug_constant = f"{_constant_name(plan.slug)}_PACKAGE_SLUG"
        name_constant = f"{_constant_name(plan.slug)}_PACKAGE_NAME"
        index_path = f"packages/{plan.slug}/src/index.ts"
        _append_symbol_pair(
            pairs,
            seen_ids,
            f"{plan.slug}-package-slug",
            f"Resolve {slug_constant}",
            slug_constant,
            index_path,
            ["synthetic", "symbol", "package", plan.slug],
        )
        _append_symbol_pair(
            pairs,
            seen_ids,
            f"{plan.slug}-package-name",
            f"Resolve {name_constant}",
            name_constant,
            index_path,
            ["synthetic", "symbol", "package", plan.slug],
        )

    for module in generated_modules:
        _append_symbol_pair(
            pairs,
            seen_ids,
            f"{module.package_slug}-{_module_suffix(module.path)}",
            f"Resolve {module.function_name}",
            module.function_name,
            module.path,
            ["synthetic", "symbol", "generated", module.package_slug],
        )

    return pairs[:50]


def _build_semantic_query_pairs(
    *,
    config: SyntheticCorpusConfig,
    route_names: list[str],
    handler_names: list[str],
    test_names: list[str],
    config_names: list[str],
    generated_modules: list[GeneratedModuleSpec],
) -> list[QueryArtifactPair]:
    pairs: list[QueryArtifactPair] = []
    fixed_pairs = [
        QueryArtifactPair(
            query=SemanticQuery(
                query_id="semantic-hero-session-invalidation",
                title="Hero semantic question",
                text="where do we invalidate sessions?",
                path_globs=["packages/**", "config/**"],
                rerank_mode=SemanticRerankMode.HYBRID,
                tags=["synthetic", "semantic", "hero", "session", "impact"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="semantic-hero-session-invalidation",
                expected_hits=[
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "invalidateSession",
                        GoldenReason.SEMANTIC,
                        3,
                    ),
                    _expected_hit(
                        "packages/api/src/routes/logout.ts",
                        "logoutRoute",
                        GoldenReason.CALLER,
                        5,
                    ),
                    _expected_hit(
                        "packages/worker/src/jobs/password-reset.ts",
                        "handlePasswordReset",
                        GoldenReason.CALLEE,
                        7,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=SemanticQuery(
                query_id="semantic-logout-entrypoint",
                title="Find the logout route entrypoint",
                text="Which route logs users out by invalidating the active session?",
                path_globs=["packages/api/**", "packages/auth/**"],
                rerank_mode=SemanticRerankMode.HYBRID,
                tags=["synthetic", "semantic", "hero", "route"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="semantic-logout-entrypoint",
                expected_hits=[
                    _expected_hit(
                        "packages/api/src/routes/logout.ts",
                        "logoutRoute",
                        GoldenReason.ROUTE,
                        3,
                    ),
                    _expected_hit(
                        "packages/api/src/handlers/logout-handler.ts",
                        "logoutHandler",
                        GoldenReason.CALLEE,
                        5,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=SemanticQuery(
                query_id="semantic-logout-handler",
                title="Find the logout handler",
                text="Which handler calls invalidateSession for the logout flow?",
                path_globs=["packages/api/**", "packages/auth/**"],
                rerank_mode=SemanticRerankMode.HYBRID,
                tags=["synthetic", "semantic", "hero", "handler"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="semantic-logout-handler",
                expected_hits=[
                    _expected_hit(
                        "packages/api/src/handlers/logout-handler.ts",
                        "logoutHandler",
                        GoldenReason.CALLEE,
                        3,
                    ),
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "invalidateSession",
                        GoldenReason.CALLEE,
                        5,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=SemanticQuery(
                query_id="semantic-password-reset-worker",
                title="Find the password reset worker",
                text="Which worker handles password reset by revoking all user sessions?",
                path_globs=["packages/worker/**", "packages/auth/**"],
                rerank_mode=SemanticRerankMode.HYBRID,
                tags=["synthetic", "semantic", "worker", "password-reset"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="semantic-password-reset-worker",
                expected_hits=[
                    _expected_hit(
                        "packages/worker/src/jobs/password-reset.ts",
                        "handlePasswordReset",
                        GoldenReason.SEMANTIC,
                        3,
                    ),
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "revokeAllUserSessions",
                        GoldenReason.CALLEE,
                        5,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=SemanticQuery(
                query_id="semantic-auth-policy-password-reset",
                title="Find the auth policy for password reset invalidation",
                text=(
                    "Where is the policy that invalidates sessions after password "
                    "reset configured?"
                ),
                path_globs=["config/**"],
                rerank_mode=SemanticRerankMode.HYBRID,
                tags=["synthetic", "semantic", "config", "hero"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="semantic-auth-policy-password-reset",
                expected_hits=[
                    _expected_hit("config/auth-policy.ts", "authPolicy", GoldenReason.CONFIG, 3)
                ],
            ),
        ),
        QueryArtifactPair(
            query=SemanticQuery(
                query_id="semantic-session-store-delete",
                title="Find session deletion storage logic",
                text="Where do we delete a session from the in-memory session store?",
                path_globs=["packages/session/**"],
                rerank_mode=SemanticRerankMode.HYBRID,
                tags=["synthetic", "semantic", "session", "storage"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="semantic-session-store-delete",
                expected_hits=[
                    _expected_hit(
                        "packages/session/src/store/session-store.ts",
                        "sessionStore",
                        GoldenReason.SEMANTIC,
                        3,
                    )
                ],
            ),
        ),
        QueryArtifactPair(
            query=SemanticQuery(
                query_id="semantic-session-invalidation-test",
                title="Find the session invalidation test",
                text="Which test covers invalidating the active session through the logout route?",
                path_globs=["packages/tests/**", "packages/auth/**", "packages/api/**"],
                rerank_mode=SemanticRerankMode.HYBRID,
                tags=["synthetic", "semantic", "tests", "hero"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="semantic-session-invalidation-test",
                expected_hits=[
                    _expected_hit(
                        "packages/tests/src/session/session.test.ts",
                        None,
                        GoldenReason.TEST,
                        3,
                    )
                ],
            ),
        ),
        QueryArtifactPair(
            query=SemanticQuery(
                query_id="semantic-revoke-all-user-sessions",
                title="Find the bulk invalidation implementation",
                text="Where do we revoke all of a user's sessions in one place?",
                path_globs=["packages/auth/**", "packages/worker/**"],
                rerank_mode=SemanticRerankMode.HYBRID,
                tags=["synthetic", "semantic", "auth", "session-bulk"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="semantic-revoke-all-user-sessions",
                expected_hits=[
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "revokeAllUserSessions",
                        GoldenReason.SEMANTIC,
                        3,
                    )
                ],
            ),
        ),
    ]
    pairs.extend(fixed_pairs)

    for route_name in _unique_preserve(["logout", *route_names[1:]]):
        pairs.append(
            QueryArtifactPair(
                query=SemanticQuery(
                    query_id=f"semantic-route-{route_name}",
                    title=f"Find the {route_name} route",
                    text=f"Where is the API route for {route_name.replace('-', ' ')}?",
                    path_globs=["packages/api/**"],
                    rerank_mode=SemanticRerankMode.HYBRID,
                    tags=["synthetic", "semantic", "route", route_name],
                    limit=10,
                ),
                expectation=GoldenExpectation(
                    query_id=f"semantic-route-{route_name}",
                    expected_hits=[
                        _expected_hit(
                            f"packages/api/src/routes/{route_name}.ts",
                            f"{_camel_case(route_name)}Route",
                            GoldenReason.ROUTE,
                            3,
                        )
                    ],
                ),
            )
        )

    for handler_name in _unique_preserve(["logout", *handler_names[1:]]):
        pairs.append(
            QueryArtifactPair(
                query=SemanticQuery(
                    query_id=f"semantic-handler-{handler_name}",
                    title=f"Find the {handler_name} handler",
                    text=f"Which handler owns the {handler_name.replace('-', ' ')} flow?",
                    path_globs=["packages/api/**"],
                    rerank_mode=SemanticRerankMode.HYBRID,
                    tags=["synthetic", "semantic", "handler", handler_name],
                    limit=10,
                ),
                expectation=GoldenExpectation(
                    query_id=f"semantic-handler-{handler_name}",
                    expected_hits=[
                        _expected_hit(
                            f"packages/api/src/handlers/{handler_name}-handler.ts",
                            f"{_camel_case(handler_name)}Handler",
                            GoldenReason.SEMANTIC,
                            3,
                        )
                    ],
                ),
            )
        )

    for file_name in config_names:
        export_name = "authPolicy" if file_name == "auth-policy.ts" else _safe_identifier(
            Path(file_name).stem
        )
        pairs.append(
            QueryArtifactPair(
                query=SemanticQuery(
                    query_id=f"semantic-config-{_slugify(Path(file_name).stem)}",
                    title=f"Find the {Path(file_name).stem} config",
                    text=(
                        "Where is the "
                        f"{Path(file_name).stem.replace('-', ' ')} configuration defined?"
                    ),
                    path_globs=["config/**"],
                    rerank_mode=SemanticRerankMode.HYBRID,
                    tags=["synthetic", "semantic", "config"],
                    limit=10,
                ),
                expectation=GoldenExpectation(
                    query_id=f"semantic-config-{_slugify(Path(file_name).stem)}",
                    expected_hits=[
                        _expected_hit(
                            f"config/{file_name}",
                            export_name,
                            GoldenReason.CONFIG,
                            3,
                        )
                    ],
                ),
            )
        )

    for test_name in test_names:
        test_path = f"packages/tests/src/session/{test_name}.test.ts"
        pairs.append(
            QueryArtifactPair(
                query=SemanticQuery(
                    query_id=f"semantic-test-{_slugify(test_name)}",
                    title=f"Find the {test_name} test",
                    text=(
                        "Which test fixture covers the "
                        f"{test_name.replace('-', ' ')} session scenario?"
                    ),
                    path_globs=["packages/tests/**"],
                    rerank_mode=SemanticRerankMode.HYBRID,
                    tags=["synthetic", "semantic", "tests"],
                    limit=10,
                ),
                expectation=GoldenExpectation(
                    query_id=f"semantic-test-{_slugify(test_name)}",
                    expected_hits=[_expected_hit(test_path, None, GoldenReason.TEST, 3)],
                ),
            )
        )

    for module in generated_modules:
        pairs.append(
            QueryArtifactPair(
                query=SemanticQuery(
                    query_id=f"semantic-{module.package_slug}-{_module_suffix(module.path)}",
                    title=(
                        f"Find generated module {Path(module.path).stem} "
                        f"in {module.package_slug}"
                    ),
                    text=(
                        "Which generated dependency summary helper belongs to the "
                        f"{module.package_slug} package for {module.function_name}?"
                    ),
                    path_globs=[f"packages/{module.package_slug}/**"],
                    rerank_mode=SemanticRerankMode.HYBRID,
                    tags=["synthetic", "semantic", "generated", module.package_slug],
                    limit=10,
                ),
                expectation=GoldenExpectation(
                    query_id=f"semantic-{module.package_slug}-{_module_suffix(module.path)}",
                    expected_hits=[
                        _expected_hit(
                            module.path,
                            module.function_name,
                            GoldenReason.SEMANTIC,
                            3,
                        )
                    ],
                ),
            )
        )

    return pairs[:30]


def _build_impact_query_pairs(
    *,
    config: SyntheticCorpusConfig,
    route_names: list[str],
    generated_modules: list[GeneratedModuleSpec],
) -> list[QueryArtifactPair]:
    pairs: list[QueryArtifactPair] = [
        QueryArtifactPair(
            query=ImpactQuery(
                query_id="impact-invalidate-session",
                title="Impact of invalidateSession behavior change",
                target_type=ImpactTargetType.SYMBOL,
                target="packages/auth/src/session/service.ts#invalidateSession",
                change_hint=ChangeHint.MODIFY_BEHAVIOR,
                tags=["synthetic", "impact", "hero", "session"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="impact-invalidate-session",
                expected_hits=[
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "invalidateSession",
                        GoldenReason.DEFINITION,
                        1,
                    ),
                    _expected_hit(
                        "packages/api/src/routes/logout.ts",
                        "logoutRoute",
                        GoldenReason.CALLER,
                        5,
                    ),
                    _expected_hit(
                        "packages/tests/src/session/session.test.ts",
                        None,
                        GoldenReason.TEST,
                        8,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=ImpactQuery(
                query_id="impact-revoke-all-user-sessions",
                title="Impact of revokeAllUserSessions signature changes",
                target_type=ImpactTargetType.SYMBOL,
                target="packages/auth/src/session/service.ts#revokeAllUserSessions",
                change_hint=ChangeHint.SIGNATURE_CHANGE,
                tags=["synthetic", "impact", "auth", "session-bulk"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="impact-revoke-all-user-sessions",
                expected_hits=[
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "revokeAllUserSessions",
                        GoldenReason.DEFINITION,
                        1,
                    ),
                    _expected_hit(
                        "packages/worker/src/jobs/password-reset.ts",
                        "handlePasswordReset",
                        GoldenReason.CALLER,
                        5,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=ImpactQuery(
                query_id="impact-auth-policy-password-reset",
                title="Impact of changing the password reset invalidation flag",
                target_type=ImpactTargetType.CONFIG_KEY,
                target="config/auth-policy.ts#invalidateOnPasswordReset",
                change_hint=ChangeHint.MODIFY_BEHAVIOR,
                tags=["synthetic", "impact", "config", "hero"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="impact-auth-policy-password-reset",
                expected_hits=[
                    _expected_hit("config/auth-policy.ts", "authPolicy", GoldenReason.CONFIG, 1),
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "invalidateSession",
                        GoldenReason.CALLEE,
                        5,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=ImpactQuery(
                query_id="impact-logout-route-file",
                title="Impact of editing the logout route file",
                target_type=ImpactTargetType.FILE,
                target="packages/api/src/routes/logout.ts",
                change_hint=ChangeHint.MODIFY_BEHAVIOR,
                tags=["synthetic", "impact", "hero", "route"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="impact-logout-route-file",
                expected_hits=[
                    _expected_hit(
                        "packages/api/src/routes/logout.ts",
                        "logoutRoute",
                        GoldenReason.DEFINITION,
                        1,
                    ),
                    _expected_hit(
                        "packages/api/src/handlers/logout-handler.ts",
                        "logoutHandler",
                        GoldenReason.CALLEE,
                        5,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=ImpactQuery(
                query_id="impact-handle-password-reset",
                title="Impact of changing the password reset worker",
                target_type=ImpactTargetType.SYMBOL,
                target="packages/worker/src/jobs/password-reset.ts#handlePasswordReset",
                change_hint=ChangeHint.MODIFY_BEHAVIOR,
                tags=["synthetic", "impact", "worker", "password-reset"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="impact-handle-password-reset",
                expected_hits=[
                    _expected_hit(
                        "packages/worker/src/jobs/password-reset.ts",
                        "handlePasswordReset",
                        GoldenReason.DEFINITION,
                        1,
                    ),
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "revokeAllUserSessions",
                        GoldenReason.CALLEE,
                        5,
                    ),
                ],
            ),
        ),
        QueryArtifactPair(
            query=ImpactQuery(
                query_id="impact-session-store",
                title="Impact of editing the session store",
                target_type=ImpactTargetType.SYMBOL,
                target="packages/session/src/store/session-store.ts#sessionStore",
                change_hint=ChangeHint.MODIFY_BEHAVIOR,
                tags=["synthetic", "impact", "session", "storage"],
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id="impact-session-store",
                expected_hits=[
                    _expected_hit(
                        "packages/session/src/store/session-store.ts",
                        "sessionStore",
                        GoldenReason.DEFINITION,
                        1,
                    ),
                    _expected_hit(
                        "packages/auth/src/session/service.ts",
                        "invalidateSession",
                        GoldenReason.CALLEE,
                        5,
                    ),
                ],
            ),
        ),
    ]

    change_hints = [
        ChangeHint.MODIFY_BEHAVIOR,
        ChangeHint.RENAME,
        ChangeHint.SIGNATURE_CHANGE,
        ChangeHint.DELETE,
    ]
    for index, module in enumerate(generated_modules):
        suffix = _module_suffix(module.path)
        pairs.append(
            QueryArtifactPair(
                query=ImpactQuery(
                    query_id=f"impact-{module.package_slug}-{suffix}",
                    title=f"Impact of changing {module.function_name}",
                    target_type=ImpactTargetType.SYMBOL,
                    target=f"{module.path}#{module.function_name}",
                    change_hint=change_hints[index % len(change_hints)],
                    tags=["synthetic", "impact", "generated", module.package_slug],
                    limit=10,
                ),
                expectation=GoldenExpectation(
                    query_id=f"impact-{module.package_slug}-{suffix}",
                    expected_hits=[
                        _expected_hit(
                            module.path,
                            module.function_name,
                            GoldenReason.DEFINITION,
                            1,
                        ),
                        _expected_hit(
                            f"packages/{module.package_slug}/src/index.ts",
                            module.function_name,
                            GoldenReason.REFERENCE,
                            4,
                        ),
                    ],
                ),
            )
        )

    return pairs[:30]


def _build_ground_truth(
    *,
    config: SyntheticCorpusConfig,
    route_names: list[str],
    handler_names: list[str],
    test_names: list[str],
    config_names: list[str],
    query_packs: list[QueryPack],
    golden_sets: list[GoldenSet],
) -> dict[str, object]:
    query_counts = {
        query_type.value: sum(
            1
            for pack in query_packs
            for query in pack.queries
            if query.type == query_type.value
        )
        for query_type in QueryType
    }
    return {
        "schema_version": "1",
        "corpus_id": config.corpus_id,
        "seed": config.seed,
        "hero_query": {
            "query": "where do we invalidate sessions?",
            "canonical_symbol": "invalidateSession",
            "canonical_path": "packages/auth/src/session/service.ts",
            "supporting_paths": [
                "packages/api/src/routes/logout.ts",
                "packages/api/src/handlers/logout-handler.ts",
                "packages/worker/src/jobs/password-reset.ts",
                "packages/tests/src/session/session.test.ts",
                "config/auth-policy.ts",
            ],
        },
        "routes": [f"packages/api/src/routes/{name}.ts" for name in route_names],
        "handlers": [f"packages/api/src/handlers/{name}-handler.ts" for name in handler_names],
        "tests": [f"packages/tests/src/session/{name}.test.ts" for name in test_names],
        "config_files": [f"config/{name}" for name in config_names],
        "query_artifacts": {
            "query_pack_ids": [pack.query_pack_id for pack in query_packs],
            "golden_set_ids": [golden.golden_set_id for golden in golden_sets],
            "query_counts": query_counts,
            "hero_query_ids": {
                "exact": "exact-invalidate-session",
                "symbol": "symbol-invalidate-session",
                "semantic": "semantic-hero-session-invalidation",
                "impact": "impact-invalidate-session",
            },
        },
        "query_expectations": {
            "semantic-hero-session-invalidation": [
                "packages/auth/src/session/service.ts",
                "packages/api/src/routes/logout.ts",
                "packages/worker/src/jobs/password-reset.ts",
            ],
            "impact-invalidate-session": [
                "packages/auth/src/session/service.ts",
                "packages/api/src/routes/logout.ts",
                "packages/tests/src/session/session.test.ts",
            ],
        },
    }


def _append_exact_pair(
    pairs: list[QueryArtifactPair],
    seen: set[tuple[str, str]],
    *,
    query_id: str,
    title: str,
    text: str,
    path: str,
    tags: list[str],
    symbol: str | None = None,
) -> None:
    key = (text, path)
    if key in seen:
        return
    seen.add(key)
    pairs.append(
        QueryArtifactPair(
            query=ExactQuery(
                query_id=query_id,
                title=title,
                text=text,
                mode=ExactMode.PLAIN,
                path_globs=[path],
                tags=_unique_preserve(tags),
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id=query_id,
                expected_hits=[_expected_hit(path, symbol, GoldenReason.TEXT, 3)],
            ),
        )
    )


def _append_symbol_pair(
    pairs: list[QueryArtifactPair],
    seen_ids: set[str],
    slug: str,
    title: str,
    symbol: str,
    path: str,
    tags: list[str],
) -> None:
    query_id = f"symbol-{slug}"
    if query_id in seen_ids:
        return
    seen_ids.add(query_id)
    top_hit = _expected_hit(path, symbol, GoldenReason.DEFINITION, 1)
    pairs.append(
        QueryArtifactPair(
            query=SymbolQuery(
                query_id=query_id,
                title=title,
                symbol=symbol,
                scope=SymbolScope.REPO,
                tags=_unique_preserve(tags),
                limit=10,
            ),
            expectation=GoldenExpectation(
                query_id=query_id,
                expected_hits=[top_hit],
                expected_top_hit=top_hit,
            ),
        )
    )


def _pairs_to_query_pack(
    config: SyntheticCorpusConfig,
    query_type: QueryType,
    pairs: list[QueryArtifactPair],
) -> QueryPack:
    return QueryPack(
        query_pack_id=f"{config.corpus_id}-{query_type.value}-pack",
        corpus_id=config.corpus_id,
        description=(
            f"Deterministic synthetic {query_type.value} queries for {config.corpus_id}."
        ),
        queries=[pair.query for pair in pairs],
    )


def _pairs_to_golden_set(
    config: SyntheticCorpusConfig,
    query_type: QueryType,
    pairs: list[QueryArtifactPair],
) -> GoldenSet:
    return GoldenSet(
        golden_set_id=f"{config.corpus_id}-{query_type.value}-goldens",
        corpus_id=config.corpus_id,
        query_pack_id=f"{config.corpus_id}-{query_type.value}-pack",
        expectations=[pair.expectation for pair in pairs],
    )


def _combine_query_packs(config: SyntheticCorpusConfig, query_packs: list[QueryPack]) -> QueryPack:
    queries = [query for pack in query_packs for query in pack.queries]
    return QueryPack(
        query_pack_id=f"{config.corpus_id}-generated-queries",
        corpus_id=config.corpus_id,
        description="Combined synthetic query pack across exact, symbol, semantic, and impact.",
        queries=queries,
    )


def _combine_golden_sets(
    config: SyntheticCorpusConfig,
    query_pack_id: str,
    golden_sets: list[GoldenSet],
) -> GoldenSet:
    expectations = [expectation for golden in golden_sets for expectation in golden.expectations]
    return GoldenSet(
        golden_set_id=f"{config.corpus_id}-generated-goldens",
        corpus_id=config.corpus_id,
        query_pack_id=query_pack_id,
        expectations=expectations,
    )


def _expected_hit(
    path: str,
    symbol: str | None,
    reason: GoldenReason,
    rank_max: int,
) -> ExpectedHit:
    return ExpectedHit(path=path, symbol=symbol, reason=reason, rank_max=rank_max)


def _build_edit_scenarios(
    config: SyntheticCorpusConfig,
    route_names: list[str],
) -> dict[str, object]:
    scenarios = [
        {
            "scenario_id": "hero-modify-invalidate-session",
            "target_path": "packages/auth/src/session/service.ts",
            "description": (
                "Modify the canonical invalidateSession behavior for incremental refresh testing."
            ),
            "before_snippet": 'return { status: "invalidated", reason };',
            "after_snippet": 'return { status: "invalidated", reason, audited: true };',
            "expected_changed_queries": [
                "exact-invalidate-session",
                "symbol-invalidate-session",
                "semantic-hero-session-invalidation",
                "impact-invalidate-session",
            ],
        },
        {
            "scenario_id": "config-toggle-password-reset",
            "target_path": "config/auth-policy.ts",
            "description": "Toggle password-reset invalidation policy for config-impact testing.",
            "before_snippet": "invalidateOnPasswordReset: true,",
            "after_snippet": "invalidateOnPasswordReset: false,",
            "expected_changed_queries": [
                "exact-auth-policy",
                "impact-auth-policy-password-reset",
            ],
        },
        {
            "scenario_id": "route-audit-logout",
            "target_path": f"packages/api/src/routes/{route_names[0]}.ts",
            "description": "Add audit logging to the hero logout route.",
            "before_snippet": "export async function logoutRoute(input: LogoutInput) {",
            "after_snippet": (
                "export async function logoutRoute(input: LogoutInput) {\n"
                '  const auditTag = "logout-route";'
            ),
            "expected_changed_queries": [
                "semantic-hero-session-invalidation",
                "impact-invalidate-session",
            ],
        },
    ]
    while len(scenarios) < config.edit_scenario_count:
        index = len(scenarios) + 1
        scenarios.append(
            {
                "scenario_id": f"generated-module-edit-{index:02d}",
                "target_path": "packages/session/src/store/session-store.ts",
                "description": (
                    "Generic generated-module edit scenario for incremental refresh tests."
                ),
                "before_snippet": "sessionCache.delete(sessionId);",
                "after_snippet": "sessionCache.delete(sessionId);\n    sessionCache.clear();",
                "expected_changed_queries": ["semantic-hero-session-invalidation"],
            }
        )
    return {"schema_version": "1", "corpus_id": config.corpus_id, "scenarios": scenarios}


def _root_package_json(config: SyntheticCorpusConfig, package_plans: list[PackagePlan]) -> str:
    return _json_text(
        {
            "name": config.corpus_id,
            "private": True,
            "packageManager": "pnpm@9.0.0",
            "workspaces": ["packages/*"],
            "scripts": {
                "build": "tsc -b",
                "test": "vitest run",
            },
            "packages": [
                f"{config.workspace_package_prefix}/{plan.slug}" for plan in package_plans
            ],
        }
    )


def _package_json(config: SyntheticCorpusConfig, plan: PackagePlan) -> str:
    dependencies = {
        f"{config.workspace_package_prefix}/{dep}": "workspace:*"
        for dep in plan.dependencies
    }
    return _json_text(
        {
            "name": f"{config.workspace_package_prefix}/{plan.slug}",
            "private": True,
            "type": "module",
            "dependencies": dependencies,
        }
    )


def _session_store_content() -> str:
    return (
        "type SessionRecord = { sessionId: string; userId: string };\n\n"
        "const sessionCache = new Map<string, SessionRecord>();\n\n"
        "export const sessionStore = {\n"
        "  delete(sessionId: string) {\n"
        "    sessionCache.delete(sessionId);\n"
        "  },\n"
        "};\n\n"
        "export function recordSessionEvent(\n"
        "  eventName: string,\n"
        "  payload: Record<string, unknown>,\n"
        ") {\n"
        "  return { eventName, payload };\n"
        "}\n"
    )


def _auth_service_content(config: SyntheticCorpusConfig) -> str:
    package_prefix = config.workspace_package_prefix
    return (
        f'import {{ sessionStore, recordSessionEvent }} from "{package_prefix}/session";\n'
        'import { authPolicy } from "../../../config/auth-policy";\n\n'
        "export type InvalidateResult = { status: \"invalidated\"; reason: string };\n\n"
        "export async function invalidateSession(sessionId: string, reason = \"logout\") "
        ": Promise<InvalidateResult> {\n"
        "  if (authPolicy.invalidateOnPasswordReset || reason !== \"password-reset\") {\n"
        "    sessionStore.delete(sessionId);\n"
        "  }\n"
        "  recordSessionEvent(\"session.invalidated\", { sessionId, reason });\n"
        '  return { status: "invalidated", reason };\n'
        "}\n\n"
        "export async function revokeAllUserSessions(userId: string, triggeredBy: string) {\n"
        "  recordSessionEvent(\"session.bulk-invalidated\", { userId, triggeredBy });\n"
        "  return `${userId}:${triggeredBy}`;\n"
        "}\n"
    )


def _handler_content(handler_name: str) -> str:
    function_name = _camel_case(handler_name) + "Handler"
    if "logout" in handler_name:
        body = '  return invalidateSession(input.sessionId, "logout");\n'
    else:
        body = '  return revokeAllUserSessions(input.userId, input.triggeredBy ?? "system");\n'
    return (
        'import { invalidateSession, revokeAllUserSessions } from "@hyperindex/auth";\n\n'
        "export type HandlerInput = {\n"
        "  sessionId: string;\n"
        "  userId: string;\n"
        "  triggeredBy?: string;\n"
        "};\n\n"
        f"export async function {function_name}(input: HandlerInput) {{\n"
        f"{body}"
        "}\n"
    )


def _route_content(route_name: str, handler_name: str) -> str:
    handler_function = _camel_case(handler_name) + "Handler"
    route_function = _camel_case(route_name) + "Route"
    return (
        f'import {{ {handler_function} }} from "../handlers/{handler_name}-handler";\n\n'
        "export type LogoutInput = { sessionId: string; userId: string };\n\n"
        f"export async function {route_function}(input: LogoutInput) {{\n"
        f"  return {handler_function}(input);\n"
        "}\n"
    )


def _worker_job_content() -> str:
    return (
        'import { revokeAllUserSessions } from "@hyperindex/auth";\n\n'
        "export async function handlePasswordReset(userId: string) {\n"
        '  return revokeAllUserSessions(userId, "password-reset-worker");\n'
        "}\n"
    )


def _hero_test_content() -> str:
    return (
        'import { invalidateSession } from "@hyperindex/auth";\n'
        'import { logoutRoute } from "@hyperindex/api";\n\n'
        'describe("session invalidation", () => {\n'
        '  it("invalidates the active session through the logout route", async () => {\n'
        '    await invalidateSession("session-1", "logout");\n'
        '    await logoutRoute({ sessionId: "session-1", userId: "user-1" });\n'
        "  });\n"
        "});\n"
    )


def _generic_test_content(test_name: str) -> str:
    title = test_name.replace("-", " ")
    return (
        'describe("generated synthetic scenario", () => {\n'
        f'  it("covers {title}", async () => {{\n'
        "    expect(true).toBe(true);\n"
        "  });\n"
        "});\n"
    )


def _generated_module_content(plan: PackagePlan, index: int) -> str:
    imports: list[str] = []
    summaries: list[str] = []
    for dependency in plan.dependencies:
        identifier = _constant_name(dependency)
        imports.append(f'import {{ {identifier}_PACKAGE_NAME }} from "@hyperindex/{dependency}";')
        summaries.append(f"{identifier}_PACKAGE_NAME")
    import_block = "\n".join(imports)
    if import_block:
        import_block += "\n\n"
    summary = ", ".join(summaries) if summaries else f'"{plan.slug}"'
    function_name = f"{_safe_identifier(plan.slug)}Generated{index + 1:03d}"
    return (
        f"{import_block}"
        f"export function {function_name}() {{\n"
        f"  const summary = [{summary}].join(\":\");\n"
        "  return summary;\n"
        "}\n"
    )


def _register_export(package_plans: list[PackagePlan], slug: str, export_path: str) -> None:
    for plan in package_plans:
        if plan.slug == slug:
            plan.exports.append(export_path)
            return
    raise SyntheticGenerationError(f"Package slug '{slug}' is not present in the synthetic plan.")


def _write_repo_files(repo_dir: Path, repo_files: dict[str, str]) -> None:
    for relative_path, content in sorted(repo_files.items()):
        path = repo_dir / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


def _write_json(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(_json_text(payload), encoding="utf-8")


def _write_query_artifacts(
    root: Path,
    artifacts: list[QueryPack] | list[GoldenSet],
) -> None:
    for artifact in artifacts:
        artifact_id = (
            artifact.query_pack_id if isinstance(artifact, QueryPack) else artifact.golden_set_id
        )
        artifact_path = root / f"{artifact_id}.json"
        _write_json(artifact_path, artifact.model_dump(mode="json"))


def _json_text(payload: object) -> str:
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def _rotate_candidates(candidates: list[str], seed: int, count: int) -> list[str]:
    if count <= 0:
        return []
    result: list[str] = []
    start = seed % len(candidates)
    cursor = start
    while len(result) < count:
        value = candidates[cursor % len(candidates)]
        if value not in result:
            result.append(value)
        else:
            result.append(f"{value}-{len(result) + 1:02d}")
        cursor += 1
    return result


def _rotated_slice(values: list[str], seed: int, count: int) -> list[str]:
    if not values or count <= 0:
        return []
    start = seed % len(values)
    rotated = values[start:] + values[:start]
    return rotated[: min(count, len(rotated))]


def _forced_dependencies(slug: str, package_names: list[str]) -> list[str]:
    forced: dict[str, list[str]] = {
        "auth": ["session"],
        "api": ["auth"],
        "worker": ["auth"],
        "tests": ["auth", "api", "worker"],
        "web": ["api", "auth"],
    }
    return [dep for dep in forced.get(slug, []) if dep in package_names]


def _unique_preserve(values: list[str]) -> list[str]:
    seen: set[str] = set()
    ordered: list[str] = []
    for value in values:
        if value not in seen:
            seen.add(value)
            ordered.append(value)
    return ordered


def _extend_names(base_names: list[str], prefix: str, count: int) -> list[str]:
    names: list[str] = []
    for index in range(count):
        if index < len(base_names):
            names.append(base_names[index])
        else:
            names.append(f"{prefix}-{index + 1:02d}")
    return names


def _camel_case(value: str) -> str:
    parts = value.replace("_", "-").split("-")
    if not parts:
        return value
    return parts[0] + "".join(part.capitalize() for part in parts[1:])


def _safe_identifier(value: str) -> str:
    return value.replace("-", "_").replace(".", "_")


def _constant_name(value: str) -> str:
    return value.replace("-", "_").upper()


def _slugify(value: str) -> str:
    return value.replace("_", "-").replace(".", "-").lower()


def _module_suffix(path: str) -> str:
    return Path(path).stem
