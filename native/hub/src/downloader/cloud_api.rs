use std::{collections::HashMap, error::Error, time::Duration};

use anyhow::{Context, Result, ensure};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue};
use rinf::{DartSignal, RustSignal};
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, error, instrument};

use crate::{
    adb::PackageName,
    models::{
        AppApiResponse, CloudApp, Popularity,
        signals::cloud_apps::{
            details::{AppDetailsResponse, GetAppDetailsRequest},
            reviews::{AppReview, AppReviewsResponse, GetAppReviewsRequest},
        },
    },
};

/// Spawns tasks answering app details and reviews requests from Dart.
pub(crate) fn start_request_handlers() {
    let client = reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    tokio::spawn({
        let client = client.clone();
        async move {
            let receiver = GetAppDetailsRequest::get_dart_signal_receiver();
            while let Some(request) = receiver.recv().await {
                tokio::spawn(send_app_details(client.clone(), request.message.package_name));
            }
        }
    });
    tokio::spawn(async move {
        let receiver = GetAppReviewsRequest::get_dart_signal_receiver();
        while let Some(request) = receiver.recv().await {
            tokio::spawn(send_app_reviews(client.clone(), request.message));
        }
    });
}

async fn send_app_details(client: reqwest::Client, package_name: String) {
    debug!(%package_name, "Received GetAppDetailsRequest");
    let package = match PackageName::parse(&package_name) {
        Ok(package) => package,
        Err(e) => {
            error!(error = e.as_ref() as &dyn Error, "Invalid package name");
            AppDetailsResponse::default_error(package_name, format!("Invalid package name: {e:#}"))
                .send_signal_to_dart();
            return;
        }
    };

    match fetch_app_details(&client, package).await {
        Ok(Some(api)) => {
            let AppApiResponse {
                id,
                display_name,
                description,
                quality_rating_aggregate,
                rating_count,
            } = api;
            AppDetailsResponse {
                package_name,
                app_id: id,
                display_name,
                description,
                rating_average: quality_rating_aggregate,
                rating_count,
                not_found: false,
                error: None,
            }
            .send_signal_to_dart();
        }
        Ok(None) => AppDetailsResponse::default_not_found(package_name).send_signal_to_dart(),
        Err(e) => {
            error!(error = e.as_ref() as &dyn Error, "Failed to fetch app details");
            AppDetailsResponse::default_error(
                package_name,
                format!("Failed to fetch app details: {e:#}"),
            )
            .send_signal_to_dart();
        }
    }
}

async fn send_app_reviews(client: reqwest::Client, request: GetAppReviewsRequest) {
    let app_id = request.app_id;
    let limit = request.limit.unwrap_or(5);
    let offset = request.offset.unwrap_or(0);
    let sort_by = request.sort_by.unwrap_or_else(|| "helpful".to_string());
    debug!(%app_id, "Received GetAppReviewsRequest");

    let response = match fetch_app_reviews(&client, &app_id, limit, offset, &sort_by).await {
        Ok(reviews) => AppReviewsResponse {
            app_id,
            total: Some(reviews.total),
            reviews: reviews.reviews,
            error: None,
        },
        Err(e) => {
            error!(error = e.as_ref() as &dyn Error, "Failed to fetch app reviews");
            AppReviewsResponse {
                app_id,
                total: None,
                reviews: Vec::new(),
                error: Some(format!("Failed to fetch reviews: {e:#}")),
            }
        }
    };
    response.send_signal_to_dart();
}

#[instrument(level = "debug", skip(client), err)]
async fn fetch_app_details(
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
struct ReviewsResponse {
    #[serde(default)]
    pub reviews: Vec<AppReview>,
    #[serde(default)]
    pub total: u32,
}

#[instrument(level = "debug", skip(client), err)]
async fn fetch_app_reviews(
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

#[derive(Deserialize, Debug)]
struct PopularityEntry {
    package_name: String,
    #[serde(rename = "1D")]
    day_1: u64,
    #[serde(rename = "7D")]
    day_7: u64,
    #[serde(rename = "30D")]
    day_30: u64,
}

/// Fetches popularity data from the QLoader API and enriches the given apps in place.
///
/// Popularity is normalized per window (1D/7D/30D) so that the most popular
/// app in each window gets 100 and others are scaled proportionally.
#[instrument(level = "debug", skip(client, apps), err)]
pub(super) async fn load_popularity_for_apps(
    client: &reqwest::Client,
    apps: &mut [CloudApp],
) -> Result<()> {
    if apps.is_empty() {
        return Ok(());
    }

    let url = "https://qloader.5698452.xyz/api/v1/popularity";
    debug!(%url, "Fetching app popularity");

    let resp = client
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to fetch popularity data")?;
    resp.error_for_status_ref().context("Failed to fetch popularity data")?;

    let mut popularity: Vec<PopularityEntry> = resp.json().await?;
    if popularity.is_empty() {
        debug!("Popularity API returned empty result");
        return Ok(());
    }

    // Filter for packages we have, so we have more relevant relative numbers
    popularity.retain(|p| apps.iter().any(|a| a.true_package_name == p.package_name));

    let mut max_1d: u64 = 0;
    let mut max_7d: u64 = 0;
    let mut max_30d: u64 = 0;
    let mut by_package: HashMap<&str, &PopularityEntry> = HashMap::new();

    for entry in &popularity {
        max_1d = max_1d.max(entry.day_1);
        max_7d = max_7d.max(entry.day_7);
        max_30d = max_30d.max(entry.day_30);
        by_package.insert(entry.package_name.as_str(), entry);
    }

    if max_1d == 0 && max_7d == 0 && max_30d == 0 {
        debug!("Popularity data only contains zeros, skipping normalization");
        return Ok(());
    }

    let normalize = |value: u64, max: u64| -> Option<u8> {
        if max == 0 || value == 0 {
            return None;
        }
        let pct = (value as f64 / max as f64 * 100.0).round();
        Some(pct.clamp(0.0, 100.0) as u8)
    };

    let mut count: u32 = 0;
    for app in apps.iter_mut() {
        if let Some(entry) = by_package.get(app.true_package_name.as_str()) {
            count += 1;
            let p1 = normalize(entry.day_1, max_1d);
            let p7 = normalize(entry.day_7, max_7d);
            let p30 = normalize(entry.day_30, max_30d);
            if p1.is_some() || p7.is_some() || p30.is_some() {
                app.popularity = Some(Popularity { day_1: p1, day_7: p7, day_30: p30 });
            }
        }
    }

    debug!(count, "Applied popularity data to app list");
    Ok(())
}

#[instrument(level = "debug", skip(client), err)]
pub(super) async fn track_download(
    client: &reqwest::Client,
    installation_id: &str,
    true_package: PackageName,
) -> Result<()> {
    let url = "https://qloader.5698452.xyz/api/v2/reportdownload";
    debug!(%url, %installation_id, %true_package, "Sending download event to QLoader API");

    let resp = client
        .post(url)
        .json(&json!({
            "installation_id": installation_id,
            "package_name": true_package.to_string(),
        }))
        .send()
        .await?;
    resp.error_for_status_ref()?;
    Ok(())
}
