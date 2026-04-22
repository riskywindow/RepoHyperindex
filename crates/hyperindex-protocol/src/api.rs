use serde::{Deserialize, Serialize};

use crate::PROTOCOL_VERSION;
use crate::buffers::{
    BufferClearParams, BufferClearResponse, BufferListParams, BufferListResponse, BufferSetParams,
    BufferSetResponse,
};
use crate::errors::ProtocolError;
use crate::impact::{
    ImpactAnalyzeParams, ImpactAnalyzeResponse, ImpactExplainParams, ImpactExplainResponse,
    ImpactStatusParams, ImpactStatusResponse,
};
use crate::planner::{
    PlannerCapabilitiesParams, PlannerCapabilitiesResponse, PlannerExplainParams,
    PlannerExplainResponse, PlannerQueryParams, PlannerQueryResponse, PlannerStatusParams,
    PlannerStatusResponse,
};
use crate::repo::{
    RepoShowParams, RepoShowResponse, RepoStatusParams, RepoStatusResponse, ReposAddParams,
    ReposAddResponse, ReposListParams, ReposListResponse, ReposRemoveParams, ReposRemoveResponse,
};
use crate::semantic::{
    SemanticBuildParams, SemanticBuildResponse, SemanticInspectChunkParams,
    SemanticInspectChunkResponse, SemanticQueryParams, SemanticQueryResponse, SemanticStatusParams,
    SemanticStatusResponse,
};
use crate::snapshot::{
    SnapshotCreateParams, SnapshotCreateResponse, SnapshotDiffParams, SnapshotDiffResponse,
    SnapshotListParams, SnapshotListResponse, SnapshotReadFileParams, SnapshotReadFileResponse,
    SnapshotShowParams, SnapshotShowResponse,
};
use crate::status::{
    DaemonStatusParams, EmptyParams, HealthResponse, RuntimeStatus, ShutdownParams,
    ShutdownResponse, VersionResponse,
};
use crate::symbols::{
    DefinitionLookupParams, DefinitionLookupResponse, ParseBuildParams, ParseBuildResponse,
    ParseInspectFileParams, ParseInspectFileResponse, ParseStatusParams, ParseStatusResponse,
    ReferenceLookupParams, ReferenceLookupResponse, SymbolIndexBuildParams,
    SymbolIndexBuildResponse, SymbolIndexStatusParams, SymbolIndexStatusResponse,
    SymbolResolveParams, SymbolResolveResponse, SymbolSearchParams, SymbolSearchResponse,
    SymbolShowParams, SymbolShowResponse,
};
use crate::watch::{
    WatchEventsParams, WatchEventsResponse, WatchStatusParams, WatchStatusResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiMethod {
    Health,
    Version,
    DaemonStatus,
    ReposAdd,
    ReposList,
    ReposRemove,
    ReposShow,
    RepoStatus,
    WatchStatus,
    WatchEvents,
    SnapshotsCreate,
    SnapshotsShow,
    SnapshotsList,
    SnapshotsDiff,
    SnapshotsReadFile,
    BuffersSet,
    BuffersClear,
    BuffersList,
    ParseBuild,
    ParseStatus,
    ParseInspectFile,
    SymbolIndexBuild,
    SymbolIndexStatus,
    SymbolSearch,
    SymbolShow,
    DefinitionLookup,
    ReferenceLookup,
    SymbolResolve,
    SemanticStatus,
    SemanticBuild,
    SemanticQuery,
    SemanticInspectChunk,
    PlannerStatus,
    PlannerQuery,
    PlannerExplain,
    PlannerCapabilities,
    ImpactStatus,
    ImpactAnalyze,
    ImpactExplain,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonRequest {
    pub protocol_version: String,
    pub request_id: String,
    #[serde(flatten)]
    pub body: RequestBody,
}

impl DaemonRequest {
    pub fn new(request_id: impl Into<String>, body: RequestBody) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_string(),
            request_id: request_id.into(),
            body,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum RequestBody {
    Health(EmptyParams),
    Version(EmptyParams),
    DaemonStatus(DaemonStatusParams),
    ReposAdd(ReposAddParams),
    ReposList(ReposListParams),
    ReposRemove(ReposRemoveParams),
    ReposShow(RepoShowParams),
    RepoStatus(RepoStatusParams),
    WatchStatus(WatchStatusParams),
    WatchEvents(WatchEventsParams),
    SnapshotsCreate(SnapshotCreateParams),
    SnapshotsShow(SnapshotShowParams),
    SnapshotsList(SnapshotListParams),
    SnapshotsDiff(SnapshotDiffParams),
    SnapshotsReadFile(SnapshotReadFileParams),
    BuffersSet(BufferSetParams),
    BuffersClear(BufferClearParams),
    BuffersList(BufferListParams),
    ParseBuild(ParseBuildParams),
    ParseStatus(ParseStatusParams),
    ParseInspectFile(ParseInspectFileParams),
    SymbolIndexBuild(SymbolIndexBuildParams),
    SymbolIndexStatus(SymbolIndexStatusParams),
    SymbolSearch(SymbolSearchParams),
    SymbolShow(SymbolShowParams),
    DefinitionLookup(DefinitionLookupParams),
    ReferenceLookup(ReferenceLookupParams),
    SymbolResolve(SymbolResolveParams),
    SemanticStatus(SemanticStatusParams),
    SemanticBuild(SemanticBuildParams),
    SemanticQuery(SemanticQueryParams),
    SemanticInspectChunk(SemanticInspectChunkParams),
    PlannerStatus(PlannerStatusParams),
    PlannerQuery(PlannerQueryParams),
    PlannerExplain(PlannerExplainParams),
    PlannerCapabilities(PlannerCapabilitiesParams),
    ImpactStatus(ImpactStatusParams),
    ImpactAnalyze(ImpactAnalyzeParams),
    ImpactExplain(ImpactExplainParams),
    Shutdown(ShutdownParams),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonResponse {
    pub protocol_version: String,
    pub request_id: String,
    #[serde(flatten)]
    pub body: ResponseBody,
}

impl DaemonResponse {
    pub fn success(request_id: impl Into<String>, result: SuccessPayload) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_string(),
            request_id: request_id.into(),
            body: ResponseBody::Success { result },
        }
    }

    pub fn error(request_id: impl Into<String>, method: ApiMethod, error: ProtocolError) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_string(),
            request_id: request_id.into(),
            body: ResponseBody::Error { method, error },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseBody {
    Success {
        #[serde(flatten)]
        result: SuccessPayload,
    },
    Error {
        method: ApiMethod,
        error: ProtocolError,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", content = "result", rename_all = "snake_case")]
pub enum SuccessPayload {
    Health(HealthResponse),
    Version(VersionResponse),
    DaemonStatus(RuntimeStatus),
    ReposAdd(ReposAddResponse),
    ReposList(ReposListResponse),
    ReposRemove(ReposRemoveResponse),
    ReposShow(RepoShowResponse),
    RepoStatus(RepoStatusResponse),
    WatchStatus(WatchStatusResponse),
    WatchEvents(WatchEventsResponse),
    SnapshotsCreate(SnapshotCreateResponse),
    SnapshotsShow(SnapshotShowResponse),
    SnapshotsList(SnapshotListResponse),
    SnapshotsDiff(SnapshotDiffResponse),
    SnapshotsReadFile(SnapshotReadFileResponse),
    BuffersSet(BufferSetResponse),
    BuffersClear(BufferClearResponse),
    BuffersList(BufferListResponse),
    ParseBuild(ParseBuildResponse),
    ParseStatus(ParseStatusResponse),
    ParseInspectFile(ParseInspectFileResponse),
    SymbolIndexBuild(SymbolIndexBuildResponse),
    SymbolIndexStatus(SymbolIndexStatusResponse),
    SymbolSearch(SymbolSearchResponse),
    SymbolShow(SymbolShowResponse),
    DefinitionLookup(DefinitionLookupResponse),
    ReferenceLookup(ReferenceLookupResponse),
    SymbolResolve(SymbolResolveResponse),
    SemanticStatus(SemanticStatusResponse),
    SemanticBuild(SemanticBuildResponse),
    SemanticQuery(SemanticQueryResponse),
    SemanticInspectChunk(SemanticInspectChunkResponse),
    PlannerStatus(PlannerStatusResponse),
    PlannerQuery(PlannerQueryResponse),
    PlannerExplain(PlannerExplainResponse),
    PlannerCapabilities(PlannerCapabilitiesResponse),
    ImpactStatus(ImpactStatusResponse),
    ImpactAnalyze(ImpactAnalyzeResponse),
    ImpactExplain(ImpactExplainResponse),
    Shutdown(ShutdownResponse),
}
