use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, DartSignal)]
pub struct GetAppReviewsRequest {
    pub app_id: String,
    /// Optional page size, defaults to 5 if None
    #[serde(default)]
    pub limit: Option<u32>,
    /// Optional offset, defaults to 0 if None
    #[serde(default)]
    pub offset: Option<u32>,
    /// Optional sort criteria, defaults to "helpful" if None. Supported: "helpful", "newest"
    #[serde(default)]
    pub sort_by: Option<String>,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct AppReviewsResponse {
    pub app_id: String,
    pub reviews: Vec<AppReview>,
    /// Total number of reviews available for the app (for pagination)
    #[serde(default)]
    pub total: Option<u32>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, SignalPiece)]
pub struct DeveloperResponse {
    pub id: String,
    pub body: String,
    #[serde(default)]
    pub date: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, SignalPiece)]
pub struct AppReview {
    pub id: String,
    pub author_display_name: Option<String>,
    #[serde(default)]
    pub author_alias: Option<String>,
    pub score: Option<f32>,
    pub review_title: Option<String>,
    pub review_description: Option<String>,
    pub date: Option<String>,
    #[serde(default)]
    pub review_helpful_count: Option<u32>,
    #[serde(default)]
    pub developer_response: Option<DeveloperResponse>,
}
