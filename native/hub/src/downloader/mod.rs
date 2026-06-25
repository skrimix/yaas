mod progress;
pub(crate) use progress::{TransferSpeedTracker, TransferStats};
mod cloud_api;
pub(crate) mod config;
pub(crate) mod controller;
pub(crate) mod download_metadata;
mod http_cache;
pub(crate) mod manager;
mod rclone;
mod repo;
mod service;
pub(crate) use service::Downloader;
pub(crate) mod downloads_catalog;
pub(crate) mod sources;

#[derive(Clone, Copy)]
pub(crate) struct SensitiveUrl<'a>(&'a str);

impl<'a> SensitiveUrl<'a> {
    pub(crate) fn new(raw: &'a str) -> Self {
        Self(raw)
    }

    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }
}

impl std::fmt::Display for SensitiveUrl<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&sanitize_url_for_log(self.0))
    }
}

impl std::fmt::Debug for SensitiveUrl<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

fn sanitize_url_for_log(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut parsed) => {
            if !parsed.username().is_empty() {
                let _ = parsed.set_username("redacted");
            }
            if parsed.password().is_some() {
                let _ = parsed.set_password(Some("redacted"));
            }
            if parsed.query().is_some() {
                parsed.set_query(Some("redacted"));
            }
            if parsed.fragment().is_some() {
                parsed.set_fragment(Some("redacted"));
            }
            parsed.to_string()
        }
        Err(_) => sanitize_raw_url_for_log(url),
    }
}

fn sanitize_raw_url_for_log(url: &str) -> String {
    let marker = url
        .find('?')
        .map(|index| (index, "?redacted"))
        .into_iter()
        .chain(url.find('#').map(|index| (index, "#redacted")))
        .min_by_key(|(index, _)| *index);

    match marker {
        Some((index, replacement)) => format!("{}{}", &url[..index], replacement),
        None => url.to_string(),
    }
}

#[derive(Debug, Clone)]
pub(crate) enum AppDownloadProgress {
    Status(String),
    Transfer(TransferStats),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_url_display_redacts_sensitive_parts() {
        assert_eq!(
            SensitiveUrl::new("https://example.com/downloader_config?Od2xfKex").to_string(),
            "https://example.com/downloader_config?redacted"
        );
        assert_eq!(
            SensitiveUrl::new("https://user:secret@example.com/path#token").to_string(),
            "https://redacted:redacted@example.com/path#redacted"
        );
    }

    #[test]
    fn sensitive_url_display_handles_unparseable_urls() {
        assert_eq!(SensitiveUrl::new("not a url?Od2xfKex").to_string(), "not a url?redacted");
    }
}
