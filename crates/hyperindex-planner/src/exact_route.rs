#[derive(Debug, Clone, Default)]
pub struct UnavailableExactRouteProvider;

impl UnavailableExactRouteProvider {
    pub fn available(&self) -> bool {
        false
    }

    pub fn unavailable_reason(&self) -> &'static str {
        "exact route remains a typed compatibility boundary in Phase 7"
    }
}
