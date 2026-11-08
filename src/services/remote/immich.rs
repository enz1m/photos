use photos_library::catalog::Photo;
use std::io::{Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use curl::easy::{Easy, Form, List};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiKey, Asset, Page, PhotoService, RemoteAlbum};

#[derive(Debug)]
pub(super) struct Immich {
    base: String,
    key: ApiKey,
}

impl Immich {
    pub fn new(base: &str, key: ApiKey) -> Result<Self> {
        ensure!(
            !key.text().trim().is_empty()
                && key.text().len() <= 4096
                && !key.text().chars().any(char::is_control),
            "Enter a valid Immich API key"
        );
        Ok(Self {
            base: super::normalized_url(base)?,
            key,
        })
    }

    fn request(
        &self,
        endpoint: &str,
        body: Option<Value>,
        form: Option<Form>,
        limit: usize,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.request_to(
            endpoint,
            body,
            form,
            cancelled,
            (&mut bytes, limit as u64),
            "POST",
        )?;
        Ok(bytes)
    }

    fn request_to(
        &self,
        endpoint: &str,
        body: Option<Value>,
        form: Option<Form>,
        cancelled: &(dyn Fn() -> bool + Sync),
        response: (&mut dyn Write, u64),
        method: &str,
    ) -> Result<()> {
        let (output, limit) = response;
        ensure!(!cancelled(), "Transfer cancelled");
        let mut easy = Easy::new();
        easy.url(&format!("{}/api/{endpoint}", self.base))?;
        easy.connect_timeout(Duration::from_secs(10))?;
        easy.timeout(Duration::from_secs(900))?;
        easy.low_speed_limit(1024)?;
        easy.low_speed_time(Duration::from_secs(30))?;
        easy.follow_location(false)?;
        easy.progress(true)?;
        let mut headers = List::new();
        headers.append(&format!("x-api-key: {}", self.key.text()))?;
        headers.append("Accept: application/json, image/*")?;
        if let Some(body) = body {
            headers.append("Content-Type: application/json")?;
            easy.post(true)?;
            easy.post_fields_copy(&serde_json::to_vec(&body)?)?;
        }
        if let Some(form) = form {
            easy.httppost(form)?;
        }
        if method == "PUT" {
            easy.custom_request("PUT")?;
        }
        easy.http_headers(headers)?;
        let mut received = 0u64;
        let mut write_error = None;
        let mut overflow = false;
        let result = {
            let mut transfer = easy.transfer();
            transfer.progress_function(|_, _, _, _| !cancelled())?;
            transfer.write_function(|chunk| {
                if cancelled() || received.saturating_add(chunk.len() as u64) > limit {
                    overflow = received.saturating_add(chunk.len() as u64) > limit;
                    return Ok(0);
                }
                if let Err(error) = output.write_all(chunk) {
                    write_error = Some(error);
                    return Ok(0);
                }
                received += chunk.len() as u64;
                Ok(chunk.len())
            })?;
            transfer.perform()
        };
        ensure!(!cancelled(), "Transfer cancelled");
        ensure!(!overflow, "Server response exceeds the transfer size limit");
        if let Some(error) = write_error {
            return Err(error.into());
        }
        result.context(
            "Cannot reach the photo service (check URL, connection and TLS certificate)",
        )?;
        match easy.response_code()? {
            200..=299 => Ok(()),
            401 | 403 => anyhow::bail!("Immich rejected the API key or its permissions"),
            300..=399 => anyhow::bail!(
                "Server redirected the request; use the final server URL (credentials were not forwarded)"
            ),
            code => anyhow::bail!("Immich request failed (HTTP {code})"),
        }
    }

    fn asset_endpoint(id: &str, suffix: &str) -> Result<String> {
        ensure!(
            !id.is_empty()
                && id.len() <= 128
                && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "Server returned an invalid asset identity"
        );
        Ok(format!("assets/{id}/{suffix}"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireAsset {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    checksum: String,
    original_file_name: String,
    #[serde(default)]
    local_date_time: String,
    exif_info: Option<Exif>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Exif {
    exif_image_width: Option<u32>,
    exif_image_height: Option<u32>,
}

impl PhotoService for Immich {
    fn check(&self, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<String> {
        let value: Value = serde_json::from_slice(&self.request(
            "users/me",
            None,
            None,
            1024 * 1024,
            cancelled,
        )?)?;
        Ok(value["name"]
            .as_str()
            .unwrap_or("Immich user")
            .chars()
            .filter(|c| !c.is_control())
            .take(100)
            .collect())
    }

    fn list(
        &self,
        cursor: Option<&str>,
        query: &str,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Page> {
        self.page(None, cursor, query, cancelled)
    }
    fn list_album(
        &self,
        album: &str,
        cursor: Option<&str>,
        query: &str,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Page> {
        Self::asset_endpoint(album, "")?;
        self.page(Some(album), cursor, query, cancelled)
    }
    fn albums(&self, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Vec<RemoteAlbum>> {
        let value: Value = serde_json::from_slice(&self.request(
            "albums",
            None,
            None,
            8 * 1024 * 1024,
            cancelled,
        )?)?;
        let items = value.as_array().context("Invalid album list")?;
        ensure!(items.len() <= 2000, "Too many albums to display");
        items.iter().map(parse_album).collect()
    }
    fn create_album(
        &self,
        name: &str,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<RemoteAlbum> {
        ensure!(
            !name.trim().is_empty()
                && name.chars().count() <= 100
                && !name.chars().any(char::is_control),
            "Invalid album name"
        );
        let value = serde_json::from_slice(&self.request(
            "albums",
            Some(json!({"albumName": name})),
            None,
            1024 * 1024,
            cancelled,
        )?)?;
        parse_album(&value)
    }
    fn upload_into(
        &self,
        photo: &Photo,
        album: Option<&str>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<bool> {
        self.upload_original(photo, album, cancelled)
            .map(|result| result.created)
    }
    fn send(
        &self,
        photo: &Photo,
        album: Option<&str>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<super::UploadReceipt> {
        self.upload_original(photo, album, cancelled)
    }
    fn download(
        &self,
        id: &str,
        output: &mut dyn Write,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<()> {
        self.request_to(
            &Self::asset_endpoint(id, "original")?,
            None,
            None,
            cancelled,
            (output, super::MAX_TRANSFER_BYTES),
            "GET",
        )
    }
    fn smart_search_available(&self, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<bool> {
        let value: Value = serde_json::from_slice(&self.request(
            "server/features",
            None,
            None,
            64 * 1024,
            cancelled,
        )?)?;
        Ok(value["smartSearch"].as_bool().unwrap_or(false))
    }
    fn search(
        &self,
        cursor: Option<&str>,
        query: &str,
        album: Option<&str>,
        kind: super::SearchKind,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<super::Page> {
        self.search_request(
            album,
            cursor,
            query,
            kind == super::SearchKind::Smart && !query.trim().is_empty(),
            cancelled,
        )
    }
    fn thumbnail(&self, id: &str, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>> {
        self.request(
            &format!("{}?size=thumbnail", Self::asset_endpoint(id, "thumbnail")?),
            None,
            None,
            8 * 1024 * 1024,
            cancelled,
        )
    }
    fn preview(&self, id: &str, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>> {
        self.request(
            &format!("{}?size=preview", Self::asset_endpoint(id, "thumbnail")?),
            None,
            None,
            8 * 1024 * 1024,
            cancelled,
        )
    }
    fn original(&self, id: &str, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>> {
        self.request(
            &Self::asset_endpoint(id, "original")?,
            None,
            None,
            photos_library::raster::MAX_ENCODED_BYTES as usize,
            cancelled,
        )
    }
    fn upload(&self, photo: &Photo, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<bool> {
        self.upload_original(photo, None, cancelled)
            .map(|result| result.created)
    }
}

fn parse_album(value: &Value) -> Result<RemoteAlbum> {
    let id = value["id"].as_str().context("Missing album identity")?;
    Immich::asset_endpoint(id, "")?;
    let name = value["albumName"].as_str().context("Missing album name")?;
    ensure!(name.len() <= 4096, "Album name too long");
    let cover = value["albumThumbnailAssetId"]
        .as_str()
        .filter(|id| !id.is_empty());
    if let Some(cover) = cover {
        Immich::asset_endpoint(cover, "")?;
    }
    Ok(RemoteAlbum {
        cover: cover.map(str::to_owned),
        id: id.into(),
        name: name.chars().filter(|c| !c.is_control()).take(100).collect(),
        count: value["assetCount"].as_u64().unwrap_or(0) as usize,
    })
}

impl Immich {
    fn page(
        &self,
        album: Option<&str>,
        cursor: Option<&str>,
        query: &str,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Page> {
        self.search_request(album, cursor, query, false, cancelled)
    }

    fn search_request(
        &self,
        album: Option<&str>,
        cursor: Option<&str>,
        query: &str,
        smart: bool,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Page> {
        let mut request = if smart {
            json!({"size":24,"withExif":true})
        } else {
            json!({"size":24,"withExif":true,"order":"desc"})
        };
        if let Some(album) = album {
            request["albumIds"] = json!([album]);
        } else {
            request["visibility"] = json!("timeline");
        }
        if let Some(cursor) = cursor {
            if let Some(page) = cursor.strip_prefix("page:") {
                let page: u32 = page.parse().context("Invalid saved page")?;
                ensure!((1..=1_000_000).contains(&page), "Invalid saved page");
                request["page"] = json!(page);
            } else {
                request["cursor"] = json!(cursor.strip_prefix("cursor:").unwrap_or(cursor));
            }
        }
        if !query.trim().is_empty() {
            request[if smart { "query" } else { "originalFileName" }] = json!(query.trim());
        }
        // Immich v3.3's filter-shaped Smart Search has no continuation cursor.
        let endpoint = if smart {
            "search/smart"
        } else {
            "search/metadata"
        };
        let value: Value = serde_json::from_slice(&self.request(
            endpoint,
            Some(request),
            None,
            8 * 1024 * 1024,
            cancelled,
        )?)?;
        let items: Vec<WireAsset> = serde_json::from_value(value["assets"]["items"].clone())
            .context("Invalid Immich photo page")?;
        ensure!(
            items.len() <= 24,
            "Immich returned too many photos in one page"
        );
        ensure!(
            items
                .iter()
                .all(|asset| Self::asset_endpoint(&asset.id, "").is_ok()
                    && asset.original_file_name.len() <= 4096
                    && asset.local_date_time.len() <= 100),
            "Immich returned oversized photo metadata"
        );
        let assets = items
            .into_iter()
            .filter(|asset| matches!(asset.kind.as_str(), "IMAGE" | "VIDEO"))
            .map(|asset| Asset {
                video: asset.kind == "VIDEO",
                checksum: asset.checksum,
                id: asset.id,
                name: asset.original_file_name,
                taken: asset.local_date_time,
                width: asset
                    .exif_info
                    .as_ref()
                    .and_then(|exif| exif.exif_image_width)
                    .unwrap_or(0),
                height: asset
                    .exif_info
                    .as_ref()
                    .and_then(|exif| exif.exif_image_height)
                    .unwrap_or(0),
            })
            .collect();
        let next = value["assets"]["nextCursor"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|cursor| format!("cursor:{cursor}"))
            .or_else(|| {
                value["assets"]["nextPage"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|page| format!("page:{page}"))
            });
        Ok(Page { assets, next })
    }

    fn upload_original(
        &self,
        photo: &Photo,
        album: Option<&str>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<super::UploadReceipt> {
        ensure!(!cancelled(), "Transfer cancelled");

        let mut file = photos_library::raster::open_media(&photo.path)?;
        let before = file.metadata()?;
        let stamp = |metadata: &std::fs::Metadata| {
            metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
        };
        ensure!(
            before.len() == photo.bytes && stamp(&before) == Some(photo.modified_ns),
            "The local original changed; rescan before uploading"
        );
        ensure!(
            before.len() <= super::MAX_TRANSFER_BYTES,
            "Photo exceeds the upload size limit"
        );
        super::reserve(&std::env::temp_dir(), before.len())?;
        let mut snapshot = tempfile::tempfile()?;
        let mut remaining = before.len();
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            ensure!(!cancelled(), "Transfer cancelled");
            let count = file.read(&mut buffer)?;
            ensure!(
                count > 0 && count as u64 <= remaining,
                "The local original changed during upload preparation"
            );
            snapshot.write_all(&buffer[..count])?;
            remaining -= count as u64;
        }
        let after = file.metadata()?;
        ensure!(
            after.len() == before.len()
                && stamp(&after) == stamp(&before)
                && after.ctime() == before.ctime()
                && after.ctime_nsec() == before.ctime_nsec(),
            "The local original changed during upload preparation"
        );
        snapshot.rewind()?;
        let now = chrono::DateTime::<chrono::Utc>::from(before.modified()?).to_rfc3339();
        let name = photo
            .path
            .file_name()
            .context("Photo has no filename")?
            .to_string_lossy();
        let mut form = Form::new();
        let snapshot_path = format!("/proc/self/fd/{}", snapshot.as_raw_fd());
        form.part("assetData")
            .file(&snapshot_path)
            .filename(name.as_ref())
            .add()?;
        form.part("fileCreatedAt").contents(now.as_bytes()).add()?;
        form.part("fileModifiedAt").contents(now.as_bytes()).add()?;
        let value: Value = serde_json::from_slice(&self.request(
            "assets",
            None,
            Some(form),
            1024 * 1024,
            cancelled,
        )?)?;
        let created = match value["status"].as_str() {
            Some("created") => true,
            Some("duplicate") => false,
            _ => anyhow::bail!("Immich did not confirm the upload"),
        };
        if let Some(album) = album {
            let id = value["id"]
                .as_str()
                .context("Upload did not return an asset identity")?;
            Self::asset_endpoint(id, "")?;
            let endpoint = Self::asset_endpoint(album, "assets")?.replacen("assets/", "albums/", 1);
            let mut response = Vec::new();
            self.request_to(
                &endpoint,
                Some(json!({"ids":[id]})),
                None,
                cancelled,
                (&mut response, 1024 * 1024),
                "PUT",
            )?;
            let results: Vec<Value> = serde_json::from_slice(&response)?;
            ensure!(
                results.len() == 1
                    && (results[0]["success"] == true || results[0]["error"] == "duplicate"),
                "Album membership was not confirmed; the uploaded original remains on the server"
            );
        }
        Ok(super::UploadReceipt {
            created,
            id: value["id"].as_str().map(str::to_owned),
        })
    }
}
