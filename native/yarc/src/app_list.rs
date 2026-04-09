use std::collections::BTreeSet;
use std::io::{self, ErrorKind};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::container::{
    CompressionScheme, FinishedYarc, PayloadKind, YarcHeader, YarcReader, YarcWriter,
};

pub const APP_LIST_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct AppRelease {
    pub app_name: String,
    pub release_name: String,
    pub package_name: String,
    pub version_code: String,
    pub megabytes: String,
    pub apk_name: String,
    pub apk_size: u64,
    pub last_modified_time: u64,
    pub manifest_hash: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
pub struct AppList {
    pub schema_version: u32,
    pub generated_at: u64,
    pub releases: Vec<AppRelease>,
}

impl AppRelease {
    pub fn validate(&self) -> io::Result<()> {
        if self.app_name.trim().is_empty() {
            return Err(invalid_data("app_release.app_name must not be empty"));
        }

        if self.release_name.trim().is_empty() {
            return Err(invalid_data("app_release.release_name must not be empty"));
        }

        if self.package_name.trim().is_empty() {
            return Err(invalid_data("app_release.package_name must not be empty"));
        }

        if self.version_code.trim().is_empty() {
            return Err(invalid_data("app_release.version_code must not be empty"));
        }

        if self.megabytes.trim().is_empty() {
            return Err(invalid_data("app_release.megabytes must not be empty"));
        }

        if self.apk_name.trim().is_empty() {
            return Err(invalid_data("app_release.apk_name must not be empty"));
        }

        validate_hash(&self.manifest_hash, "app_release.manifest_hash")?;
        Ok(())
    }
}

impl AppList {
    pub fn validate(&self) -> io::Result<()> {
        if self.schema_version != APP_LIST_SCHEMA_VERSION {
            return Err(invalid_data("unsupported app list schema version"));
        }

        let mut seen_release_names = BTreeSet::new();
        for release in &self.releases {
            release.validate()?;

            if !seen_release_names.insert(release.release_name.clone()) {
                return Err(invalid_data(
                    "app list contains duplicate release_name entries",
                ));
            }
        }

        Ok(())
    }

    pub fn to_json_bytes(&self) -> io::Result<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|err| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!("failed to encode app list json: {err}"),
            )
        })
    }

    pub fn from_json_bytes(bytes: &[u8]) -> io::Result<Self> {
        let app_list: Self = serde_json::from_slice(bytes).map_err(|err| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!("failed to decode app list json: {err}"),
            )
        })?;
        app_list.validate()?;
        Ok(app_list)
    }

    pub async fn to_yarc<W: AsyncWrite + Unpin>(
        &self,
        out: W,
        key_bytes: [u8; 32],
        chunk_log2: u8,
    ) -> io::Result<FinishedYarc<W>> {
        let bytes = self.to_json_bytes()?;
        let writer = YarcWriter::new(key_bytes, chunk_log2);
        writer
            .encrypt_bytes_with_compression(
                PayloadKind::AppList,
                &bytes,
                out,
                CompressionScheme::Zstd,
            )
            .await
    }

    pub async fn from_yarc<R: AsyncRead + Unpin>(
        input: R,
        key_bytes: [u8; 32],
    ) -> io::Result<(Self, YarcHeader)> {
        let reader = YarcReader::new(key_bytes);
        let (bytes, header) = reader.decrypt_to_bytes(input, PayloadKind::AppList).await?;
        let app_list = Self::from_json_bytes(&bytes)?;
        Ok((app_list, header))
    }
}

fn validate_hash(hash: &str, field_name: &str) -> io::Result<()> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(invalid_data(&format!(
            "{field_name} must be a 64-character lowercase hex digest",
        )));
    }

    if !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_data(&format!(
            "{field_name} must be a 64-character lowercase hex digest",
        )));
    }

    Ok(())
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_list_rejects_duplicate_release_names() {
        let app_release = sample_app_release("sample-release");
        let app_list = AppList {
            schema_version: APP_LIST_SCHEMA_VERSION,
            generated_at: 456,
            releases: vec![app_release.clone(), app_release],
        };

        let err = app_list
            .validate()
            .expect_err("duplicate release names should fail validation");
        assert_eq!(err.kind(), ErrorKind::InvalidData);
        assert_eq!(
            err.to_string(),
            "app list contains duplicate release_name entries"
        );
    }

    #[tokio::test]
    async fn app_list_round_trips_through_yarc() -> io::Result<()> {
        let app_list = AppList {
            schema_version: APP_LIST_SCHEMA_VERSION,
            generated_at: 999,
            releases: vec![sample_app_release("sample-release")],
        };

        let yarc = app_list.to_yarc(Vec::new(), [12; 32], 12).await?;
        let (decoded, header) = AppList::from_yarc(yarc.out.as_slice(), [12; 32]).await?;

        assert_eq!(header.kind(), PayloadKind::AppList);
        assert_eq!(header.version(), 2);
        assert_eq!(header.compression_scheme(), CompressionScheme::Zstd);
        assert_eq!(decoded, app_list);
        Ok(())
    }

    fn sample_app_release(release_name: &str) -> AppRelease {
        AppRelease {
            app_name: "Sample App".to_string(),
            release_name: release_name.to_string(),
            package_name: "com.example.sample".to_string(),
            version_code: "123".to_string(),
            megabytes: "321".to_string(),
            apk_name: "sample.apk".to_string(),
            apk_size: 456,
            last_modified_time: 789,
            manifest_hash: "d".repeat(64),
        }
    }
}
