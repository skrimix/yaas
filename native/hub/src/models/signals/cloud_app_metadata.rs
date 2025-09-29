use rinf::{DartSignal, RustSignal, SignalPiece};
use serde::{Deserialize, Serialize};

use crate::models::CloudApp;

#[derive(Serialize, Deserialize, DartSignal)]
pub struct LoadCloudAppsRequest {
    pub refresh: bool,
}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct CloudAppsChangedEvent {
    pub apps: Vec<CloudApp>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct GetRcloneRemotesRequest {}

#[derive(Serialize, Deserialize, RustSignal)]
pub struct RcloneRemotesChanged {
    pub remotes: Vec<String>,
    pub error: Option<String>,
}

// Request detailed info about an app from the external API by package name
#[derive(Serialize, Deserialize, DartSignal)]
pub struct GetAppDetailsRequest {
    pub package_name: String,
}

// Response with app details fetched from the external API.
#[derive(Serialize, Deserialize, RustSignal)]
pub struct AppDetailsResponse {
    pub package_name: String,
    pub app_id: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub rating_average: Option<f32>,
    pub rating_count: Option<u32>,
    /// True if the app was not found (HTTP 404)
    pub not_found: bool,
    /// Error message for non-404 errors
    pub error: Option<String>,
}

impl AppDetailsResponse {
    pub fn default_not_found(package_name: String) -> Self {
        Self {
            package_name,
            app_id: None,
            display_name: None,
            description: None,
            rating_average: None,
            rating_count: None,
            not_found: true,
            error: None,
        }
    }

    pub fn default_error(package_name: String, error: String) -> Self {
        Self {
            package_name,
            app_id: None,
            display_name: None,
            description: None,
            rating_average: None,
            rating_count: None,
            not_found: false,
            error: Some(error),
        }
    }
}

#[derive(Serialize, Deserialize, DartSignal)]
pub struct GetAppReviewsRequest {
    pub app_id: String,
    /// Optional page size; defaults to 5 if None
    #[serde(default)]
    pub limit: Option<u32>,
    /// Optional offset; defaults to 0 if None
    #[serde(default)]
    pub offset: Option<u32>,
    /// Optional sort criteria; defaults to "helpful" if None. Supported: "helpful", "newest"
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
