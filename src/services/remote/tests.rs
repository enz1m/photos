use photos_library::catalog::Photo;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpListener;
use std::path::Path;

use super::*;

#[derive(Debug)]
struct Fixture(Vec<u8>);
impl PhotoService for Fixture {
    fn check(&self, _: &(dyn Fn() -> bool + Sync)) -> Result<String> {
        Ok("test".into())
    }
    fn list(&self, _: Option<&str>, _: &str, _: &(dyn Fn() -> bool + Sync)) -> Result<Page> {
        anyhow::bail!("unused")
    }
    fn thumbnail(&self, _: &str, _: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>> {
        Ok(self.0.clone())
    }
    fn original(&self, _: &str, _: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>> {
        Ok(self.0.clone())
    }
    fn upload(&self, _: &Photo, _: &(dyn Fn() -> bool + Sync)) -> Result<bool> {
        anyhow::bail!("unused")
    }
}

fn png() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(8, 6, image::Rgba([21, 88, 144, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("PNG");
    bytes.into_inner()
}

fn asset(name: &str) -> Asset {
    Asset {
        video: false,
        checksum: String::new(),
        id: "asset-1".into(),
        name: name.into(),
        taken: String::new(),
        width: 8,
        height: 6,
    }
}

#[test]
fn service_import_publishes_distinct_originals_and_refuses_unsafe_or_invalid_input() {
    let sandbox = tempfile::tempdir().expect("sandbox");
    let provider = Fixture(png());
    let existing = sandbox.path().join("photo.png");
    std::fs::write(&existing, b"existing user data").expect("existing");
    let copy = import_original(
        &provider,
        &asset("photo.png"),
        sandbox.path(),
        false,
        &|| false,
    )
    .expect("new copy");
    assert_ne!(copy, existing);
    assert_eq!(
        std::fs::read(&existing).expect("old"),
        b"existing user data"
    );
    assert_eq!(std::fs::read(&copy).expect("copy"), provider.0);
    let count = std::fs::read_dir(sandbox.path()).expect("entries").count();
    for name in [
        "../escape.png",
        "/tmp/escape.png",
        "..\\escape.png",
        "bad\n.png",
    ] {
        assert!(
            import_original(&provider, &asset(name), sandbox.path(), false, &|| false).is_err()
        );
    }
    assert!(
        import_original(
            &Fixture(b"not an image".to_vec()),
            &asset("bad.png"),
            sandbox.path(),
            false,
            &|| false
        )
        .is_err()
    );
    assert!(
        import_original(
            &provider,
            &asset("cancelled.png"),
            sandbox.path(),
            false,
            &|| true
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read_dir(sandbox.path()).expect("entries").count(),
        count
    );
    let dated = Asset {
        taken: "2024-02-03T12:30:00Z".into(),
        ..asset("dated.png")
    };
    let dated_path =
        import_original(&provider, &dated, sandbox.path(), true, &|| false).expect("dated import");
    assert_eq!(
        dated_path
            .strip_prefix(sandbox.path())
            .expect("inside destination"),
        Path::new("2024/02/dated.png")
    );
    assert_eq!(
        std::fs::metadata(&dated_path)
            .expect("metadata")
            .modified()
            .expect("date")
            .duration_since(std::time::UNIX_EPOCH)
            .expect("epoch")
            .as_secs(),
        1706963400
    );
    let grouped = import_batch(
        &provider,
        &[asset("one.png"), asset("two.png")],
        sandbox.path(),
        photos_library::catalog::ImportLayout::PerImport,
        "Home / server",
        &|| false,
    )
    .expect("grouped imports");
    assert_eq!(grouped.paths.len(), 2);
    assert_eq!(grouped.paths[0].parent(), grouped.paths[1].parent());
    assert_eq!(
        grouped.paths[0].parent().and_then(Path::parent),
        Some(sandbox.path())
    );
}

#[test]
fn service_upload_refuses_replaced_sources_before_contacting_a_server() {
    let sandbox = tempfile::tempdir().expect("sandbox");
    let source = sandbox.path().join("original.png");
    std::fs::write(&source, png()).expect("source");
    let photo = photos_library::scan::inspect_file(&source).expect("photo");
    let account = ServiceAccount {
        id: 1,
        name: "unreachable".into(),
        provider: ServiceProvider::Immich,
        url: "http://127.0.0.1:1".into(),
    };
    let provider = connect(&account, ApiKey::new("fixture-key".into())).expect("client");
    std::fs::write(&source, b"replacement").expect("replace");
    assert!(
        provider
            .upload(&photo, &|| false)
            .expect_err("changed source")
            .to_string()
            .contains("changed")
    );
    #[cfg(unix)]
    {
        std::fs::remove_file(&source).expect("remove fixture");
        let target = sandbox.path().join("target.png");
        std::fs::write(&target, png()).expect("target");
        std::os::unix::fs::symlink(&target, &source).expect("symlink");
        assert!(provider.upload(&photo, &|| false).is_err());
        assert_eq!(std::fs::read(target).expect("preserved target"), png());
    }
}

#[test]
fn service_urls_and_credentials_are_not_redirected_or_exposed() {
    for value in [
        "file:///etc/passwd",
        "https://user:secret@example.test",
        "https://example.test/?key=secret",
        "https://example.test/#secret",
    ] {
        assert!(normalized_url(value).is_err());
    }
    assert_eq!(
        normalized_url("https://example.test/photos/api/").expect("URL"),
        "https://example.test/photos"
    );
    let key = ApiKey::new("fixture-private-key".into());
    assert!(!format!("{key:?}").contains("fixture-private-key"));
    let listener = TcpListener::bind("127.0.0.1:0").expect("server");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let destination = TcpListener::bind("127.0.0.1:0").expect("redirect server");
    destination.set_nonblocking(true).expect("nonblocking");
    let target = destination.local_addr().expect("target");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("request");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("timeout");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("header");
            if line == "\r\n" {
                break;
            }
            headers.push_str(&line);
        }
        write!(stream,"HTTP/1.1 302 Found\r\nLocation: http://{target}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").expect("reply");
        headers
    });
    let account = ServiceAccount {
        id: 1,
        name: "fixture".into(),
        provider: ServiceProvider::Immich,
        url,
    };
    let error = connect(&account, key)
        .expect("client")
        .check(&|| false)
        .expect_err("redirect rejected");
    assert!(error.to_string().contains("redirected"));
    assert!(
        server
            .join()
            .expect("server")
            .contains("x-api-key: fixture-private-key")
    );
    assert!(
        matches!(destination.accept(),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
    );
}

#[test]
#[ignore = "requires the isolated Docker Immich; set ENZ_PHOTOS_IMMICH_TEST_CONFIG and run --ignored"]
fn immich_live_upload_browse_preview_download_duplicate_and_auth_roundtrip() {
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::env::var("ENZ_PHOTOS_IMMICH_TEST_CONFIG").expect("test config path"))
            .expect("private config"),
    )
    .expect("config");
    let account = ServiceAccount {
        id: 1,
        name: "integration".into(),
        provider: ServiceProvider::Immich,
        url: config["url"].as_str().expect("url").into(),
    };
    let provider = connect(
        &account,
        ApiKey::new(config["api_key"].as_str().expect("key").into()),
    )
    .expect("client");
    assert!(
        !provider
            .check(&|| false)
            .expect("authenticated user")
            .is_empty()
    );
    let sandbox = tempfile::tempdir().expect("sandbox");
    let path = sandbox.path().join(format!(
        "Photos roundtrip {}.png",
        chrono::Utc::now().timestamp_nanos_opt().expect("timestamp")
    ));
    let mut bytes = png();
    bytes.extend_from_slice(
        &chrono::Utc::now()
            .timestamp_nanos_opt()
            .expect("timestamp")
            .to_le_bytes(),
    );
    std::fs::write(&path, &bytes).expect("source");
    let photo = photos_library::scan::inspect_file(&path).expect("source metadata");
    assert!(provider.upload(&photo, &|| false).expect("upload"));
    assert!(!provider.upload(&photo, &|| false).expect("duplicate"));
    let page = provider
        .list(
            None,
            path.file_name().expect("filename").to_str().expect("UTF8"),
            &|| false,
        )
        .expect("search");
    let asset = page.assets.first().expect("uploaded asset");
    assert_eq!(
        asset.name,
        path.file_name().expect("filename").to_string_lossy()
    );
    assert_eq!(
        provider.original(&asset.id, &|| false).expect("original"),
        bytes
    );
    let started = std::time::Instant::now();
    loop {
        if let Ok(preview) = provider.thumbnail(&asset.id, &|| false) {
            assert!(photos_library::raster::decode(&preview, || false).is_ok());
            break;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "Immich did not generate a thumbnail"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let output = sandbox.path().join("imports");
    let imported =
        import_original(provider.as_ref(), asset, &output, false, &|| false).expect("import");
    assert_eq!(std::fs::read(imported).expect("imported"), bytes);
    assert_eq!(std::fs::read(&path).expect("unchanged source"), bytes);
    let album = provider
        .create_album(
            &format!(
                "Photos review {}",
                chrono::Utc::now().timestamp_nanos_opt().expect("stamp")
            ),
            &|| false,
        )
        .expect("create album");
    assert!(
        !provider
            .upload_into(&photo, Some(&album.id), &|| false)
            .expect("existing photo added to album")
    );
    assert!(
        provider
            .albums(&|| false)
            .expect("albums")
            .iter()
            .any(|a| a.id == album.id)
    );
    assert_eq!(
        provider
            .list_album(&album.id, None, "", &|| false)
            .expect("album page")
            .assets
            .len(),
        1
    );
    let video = sandbox.path().join(format!(
        "Review video {}.mp4",
        chrono::Utc::now().timestamp_nanos_opt().expect("stamp")
    ));
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=320x240:rate=12",
                "-t",
                "2",
                "-threads",
                "1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p"
            ])
            .arg(&video)
            .status()
            .expect("ffmpeg")
            .success()
    );
    let mut large = std::fs::OpenOptions::new()
        .append(true)
        .open(&video)
        .expect("clip padding");
    large
        .write_all(
            &chrono::Utc::now()
                .timestamp_nanos_opt()
                .expect("unique video")
                .to_le_bytes(),
        )
        .expect("unique source bytes");
    large
        .set_len(photos_library::raster::MAX_ENCODED_BYTES + 1024 * 1024)
        .expect("large video");
    drop(large);
    let clip = photos_library::scan::inspect_file(&video).expect("large video metadata");
    let clip_hash = sync::digest(&video, &|| false).expect("original clip hash");
    assert!(
        provider
            .upload_into(&clip, Some(&album.id), &|| false)
            .expect("video upload")
    );
    assert!(
        !provider
            .upload_into(&clip, Some(&album.id), &|| false)
            .expect("duplicate video membership")
    );
    let album_page = provider
        .list_album(&album.id, None, "", &|| false)
        .expect("mixed album");
    let remote_video = album_page
        .assets
        .iter()
        .find(|a| a.video)
        .expect("video visible");
    let imported_video =
        import_original(provider.as_ref(), remote_video, &output, false, &|| false)
            .expect("video import");
    assert_eq!(
        sync::digest(&imported_video, &|| false).expect("downloaded clip hash"),
        clip_hash
    );
    assert_eq!(
        sync::digest(&video, &|| false).expect("unchanged clip hash"),
        clip_hash
    );
    let frame = crate::services::video::frame(&imported_video, 0.5, Some(160), &|| false)
        .expect("imported video frame");
    assert_eq!(frame.dimensions(), (160, 120));
    assert!(import_original(provider.as_ref(), remote_video, &output, false, &|| true).is_err());
    let sync_folder = sandbox.path().join("sync-media");
    std::fs::create_dir(&sync_folder).expect("sync folder");
    let local = sync_folder.join("local.png");
    let mut local_bytes = png();
    local_bytes.extend_from_slice(b"unique sync original");
    local_bytes.extend_from_slice(
        &chrono::Utc::now()
            .timestamp_nanos_opt()
            .expect("stamp")
            .to_le_bytes(),
    );
    std::fs::write(&local, &local_bytes).expect("local sync source");
    let plan = photos_library::catalog::ServiceSync {
        folder: sync_folder.clone(),
        direction: photos_library::catalog::SyncDirection::Both,
        album: Some(album.id.clone()),
    };
    let history = sandbox.path().join("sync.json");
    let first = sync::run(provider.as_ref(), &plan, &history, &|| false);
    assert!(first.error.is_none(), "{:?}", first.error);
    assert_eq!(first.downloaded, 2);
    assert_eq!(first.uploaded, 1);
    let retry = sync::run(provider.as_ref(), &plan, &history, &|| false);
    assert!(retry.error.is_none(), "{:?}", retry.error);
    assert_eq!((retry.downloaded, retry.uploaded), (0, 0));
    assert_eq!(std::fs::read(&local).expect("local preserved"), local_bytes);
    assert_eq!(
        provider
            .list_album(&album.id, None, "", &|| false)
            .expect("synced album")
            .assets
            .len(),
        3
    );
    let edited = first
        .paths
        .iter()
        .find(|p| p.extension().is_some_and(|e| e == "png"))
        .expect("downloaded photo");
    let mut edited_bytes = png();
    edited_bytes.extend_from_slice(b"local edited copy");
    edited_bytes.extend_from_slice(
        &chrono::Utc::now()
            .timestamp_nanos_opt()
            .expect("stamp")
            .to_le_bytes(),
    );
    std::fs::write(edited, &edited_bytes).expect("local edit");
    let changed = sync::run(provider.as_ref(), &plan, &history, &|| false);
    assert!(changed.error.is_none(), "{:?}", changed.error);
    assert_eq!(changed.uploaded, 1);
    assert_eq!(
        provider
            .original(&asset.id, &|| false)
            .expect("remote original preserved"),
        bytes
    );
    assert_eq!(
        std::fs::read(edited).expect("local edit preserved"),
        edited_bytes
    );
    let all = provider.list(None, "", &|| false).expect("first page");
    assert_eq!(all.assets.len(), 24);
    let next = provider
        .list(all.next.as_deref(), "", &|| false)
        .expect("next page");
    assert!(!next.assets.is_empty());
    assert!(
        !next
            .assets
            .iter()
            .any(|b| all.assets.iter().any(|a| a.id == b.id))
    );
    assert!(
        connect(&account, ApiKey::new("invalid-key".into()))
            .expect("bad client")
            .check(&|| false)
            .is_err()
    );
}

#[derive(Debug)]
struct SyncFixture {
    media: std::sync::Mutex<Vec<Asset>>,
    fail: std::sync::atomic::AtomicBool,
}
impl PhotoService for SyncFixture {
    fn check(&self, _: &(dyn Fn() -> bool + Sync)) -> Result<String> {
        Ok("fixture".into())
    }
    fn list(&self, _: Option<&str>, _: &str, _: &(dyn Fn() -> bool + Sync)) -> Result<Page> {
        Ok(Page {
            assets: self.media.lock().expect("media").clone(),
            next: None,
        })
    }
    fn thumbnail(&self, _: &str, _: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>> {
        Ok(png())
    }
    fn original(&self, _: &str, _: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>> {
        Ok(png())
    }
    fn download(
        &self,
        _: &str,
        output: &mut dyn Write,
        _: &(dyn Fn() -> bool + Sync),
    ) -> Result<()> {
        if self.fail.load(std::sync::atomic::Ordering::Acquire) {
            output.write_all(b"incomplete")?;
            anyhow::bail!("Interrupted fixture download");
        }
        output.write_all(&png())?;
        Ok(())
    }
    fn upload(&self, _: &Photo, _: &(dyn Fn() -> bool + Sync)) -> Result<bool> {
        anyhow::bail!("Downloads must not upload anything")
    }
}

#[test]
fn sync_retries_keep_local_edits_and_deletions_and_never_clobber_a_collision() {
    use photos_library::catalog::{ServiceSync, SyncDirection};
    let sandbox = tempfile::tempdir().expect("sandbox");
    let folder = sandbox.path().join("media");
    std::fs::create_dir(&folder).expect("folder");
    let original = folder.join("same.png");
    std::fs::write(&original, b"existing original").expect("original");
    let provider = SyncFixture {
        media: std::sync::Mutex::new(vec![
            Asset {
                id: "one".into(),
                checksum: "one".into(),
                ..asset("same.png")
            },
            Asset {
                id: "two".into(),
                checksum: "two".into(),
                ..asset("same.png")
            },
        ]),
        fail: false.into(),
    };
    let plan = ServiceSync {
        folder: folder.clone(),
        direction: SyncDirection::Download,
        album: None,
    };
    let state = sandbox.path().join("sync.json");
    let first = sync::run(&provider, &plan, &state, &|| false);
    assert!(first.error.is_none(), "{:?}", first.error);
    assert_eq!(first.downloaded, 2);
    assert_ne!(first.paths[0], first.paths[1]);
    assert_eq!(
        std::fs::read(&original).expect("untouched"),
        b"existing original"
    );
    let recovered = sync::run(&provider, &plan, &state, &|| false);
    assert_eq!(recovered.downloaded, 0);
    assert_eq!(recovered.paths.len(), 2);
    let mut pending: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state).expect("history")).expect("JSON");
    pending["downloaded"]
        .as_object_mut()
        .expect("receipts")
        .remove("one");
    std::fs::write(&state, serde_json::to_vec(&pending).expect("encode"))
        .expect("simulate missing checkpoint");
    let resumed = sync::run(&provider, &plan, &state, &|| false);
    assert!(resumed.error.is_none());
    assert_eq!(resumed.downloaded, 0);
    assert_eq!(resumed.paths.len(), 2);
    assert_eq!(std::fs::read_dir(&folder).expect("entries").count(), 3);
    std::fs::write(&first.paths[0], b"local edit").expect("edit");
    std::fs::remove_file(&first.paths[1]).expect("local deletion");
    provider.media.lock().expect("media")[0].checksum = "remote changed".into();
    let again = sync::run(&provider, &plan, &state, &|| false);
    assert!(again.error.is_none());
    assert_eq!(again.downloaded, 0);
    assert_eq!(again.conflicts, 1);
    assert_eq!(
        std::fs::read(&first.paths[0]).expect("edit retained"),
        b"local edit"
    );
    assert!(!first.paths[1].exists());
    let mut history: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state).expect("history")).expect("history JSON");
    history["downloaded"]
        .as_object_mut()
        .expect("receipts")
        .remove("one");
    std::fs::write(&state, serde_json::to_vec(&history).expect("encode"))
        .expect("simulate publication before checkpoint");
    provider.media.lock().expect("media")[0].checksum = "one".into();
    let collision = sync::run(&provider, &plan, &state, &|| false);
    assert!(collision.error.is_none());
    assert_eq!(collision.downloaded, 0);
    assert_eq!(collision.conflicts, 1);
    assert_eq!(
        std::fs::read(&first.paths[0]).expect("collision retained"),
        b"local edit"
    );
    assert_eq!(std::fs::read_dir(&folder).expect("entries").count(), 2);
}

#[test]
fn sync_interruption_folder_replacement_and_competing_owner_stop_without_partial_publication() {
    use photos_library::catalog::{ServiceSync, SyncDirection};
    let sandbox = tempfile::tempdir().expect("sandbox");
    let folder = sandbox.path().join("media");
    std::fs::create_dir(&folder).expect("folder");
    let state = sandbox.path().join("sync.json");
    let plan = ServiceSync {
        folder: folder.clone(),
        direction: SyncDirection::Download,
        album: None,
    };
    let provider = SyncFixture {
        media: std::sync::Mutex::new(vec![asset("one.png")]),
        fail: true.into(),
    };
    let failed = sync::run(&provider, &plan, &state, &|| false);
    assert!(failed.error.is_some());
    assert!(failed.paths.is_empty());
    assert_eq!(std::fs::read_dir(&folder).expect("entries").count(), 0);
    provider
        .fail
        .store(false, std::sync::atomic::Ordering::Release);
    let resumed = sync::run(&provider, &plan, &state, &|| false);
    assert!(resumed.error.is_none());
    assert_eq!(resumed.downloaded, 1);
    let saved = std::fs::read(&state).expect("saved");
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state.with_extension("lock"))
        .expect("lease");
    lease.try_lock().expect("own lease");
    assert!(
        sync::run(&provider, &plan, &state, &|| false)
            .error
            .expect("owner rejected")
            .contains("Another Photos")
    );
    drop(lease);
    assert_eq!(sync::run(&provider, &plan, &state, &|| true).downloaded, 0);
    std::fs::rename(&folder, sandbox.path().join("retained")).expect("move folder");
    std::fs::create_dir(&folder).expect("replacement");
    assert!(
        sync::run(&provider, &plan, &state, &|| false)
            .error
            .expect("replacement rejected")
            .contains("identity changed")
    );
    assert_eq!(std::fs::read(&state).expect("unchanged"), saved);
    assert_eq!(
        std::fs::read_dir(&folder)
            .expect("replacement entries")
            .count(),
        0
    );
}

#[test]
fn service_smart_search_uses_content_queries_and_keeps_album_and_continuation_scope() {
    use std::io::Read;
    let listener = TcpListener::bind("127.0.0.1:0").expect("server");
    let account = ServiceAccount {
        id: 1,
        name: "fixture".into(),
        provider: ServiceProvider::Immich,
        url: format!("http://{}", listener.local_addr().expect("address")),
    };
    let server = std::thread::spawn(move || {
        let mut received = Vec::new();
        for index in 0..4 {
            let (mut stream, _) = listener.accept().expect("request");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("timeout");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut first = String::new();
            reader.read_line(&mut first).expect("request line");
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("header");
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().expect("length");
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).expect("body");
            received.push((
                first,
                if body.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&body).expect("JSON")
                },
            ));
            let response = if index == 0 {
                r#"{"smartSearch":true}"#
            } else {
                r#"{"assets":{"items":[],"nextPage":"3"}}"#
            };
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).expect("reply");
        }
        received
    });
    let client = connect(&account, ApiKey::new("fixture-key".into())).expect("client");
    assert!(client.smart_search_available(&|| false).expect("features"));
    let smart = client
        .search(
            Some("page:2"),
            "a sailboat at sea",
            Some("album-1"),
            SearchKind::Smart,
            &|| false,
        )
        .expect("AI search");
    assert_eq!(smart.next.as_deref(), Some("page:3"));
    client
        .search(None, "IMG_123", None, SearchKind::Filename, &|| false)
        .expect("filename search");
    client
        .search(None, "", None, SearchKind::Smart, &|| false)
        .expect("empty search uses timeline");
    let requests = server.join().expect("server");
    assert!(requests[0].0.starts_with("GET /api/server/features "));
    assert!(requests[1].0.starts_with("POST /api/search/smart "));
    assert_eq!(requests[1].1["query"], "a sailboat at sea");
    assert_eq!(requests[1].1["page"], 2);
    assert_eq!(requests[1].1["albumIds"], serde_json::json!(["album-1"]));
    assert!(requests[1].1.get("originalFileName").is_none());
    assert!(requests[1].1.get("order").is_none());
    assert!(requests[2].0.starts_with("POST /api/search/metadata "));
    assert_eq!(requests[2].1["originalFileName"], "IMG_123");
    assert!(requests[3].0.starts_with("POST /api/search/metadata "));
}
