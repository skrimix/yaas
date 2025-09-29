use rinf::RustSignal;
use rinf::DartSignal;
use serde::{Deserialize, Serialize};

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

