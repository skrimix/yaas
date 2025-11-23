use anyhow::{Result, ensure};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use tracing::{debug, instrument};

use crate::{
    adb::PackageName,
    models::{AppApiResponse, signals::cloud_apps::reviews::AppReview},
};

#[instrument(level = "debug", skip(client), err)]
pub(super) async fn fetch_app_details(
    client: &reqwest::Client,
    package: PackageName,
) -> Result<Option<AppApiResponse>> {
    let url = format!("https://qloader.5698452.xyz/api/v1/oculusgames/{package}");
    debug!(%url, "Fetching app details from QLoader API");

    let resp = client.get(&url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    resp.error_for_status_ref()?;

    let api: AppApiResponse = resp.json().await?;
    Ok(Some(api))
}

#[derive(serde::Deserialize)]
pub(super) struct ReviewsResponse {
    #[serde(default)]
    pub reviews: Vec<AppReview>,
    #[serde(default)]
    pub total: u32,
}

#[instrument(level = "debug", skip(client), err)]
pub(super) async fn fetch_app_reviews(
    client: &reqwest::Client,
    app_id: &str,
    limit: u32,
    offset: u32,
    sort_by: &str,
) -> Result<ReviewsResponse> {
    ensure!(sort_by == "helpful" || sort_by == "newest", "Invalid sort_by value: {}", sort_by);
    debug!(%app_id, %limit, %offset, %sort_by, "Fetching app reviews");
    let url = "https://reviews.5698452.xyz";

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

    let response = client
        .get(url)
        .headers(headers)
        .query(&[
            ("appId", app_id),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
            ("sortBy", sort_by),
        ])
        .send()
        .await?;

    response.error_for_status_ref()?;
    let payload: ReviewsResponse = response.json().await?;
    Ok(payload)
}
