use std::ffi::OsString;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use enzui_iced_core::cancellation::{CancelCause, CancelToken};
use enzui_iced_core::user_file::{self, SavePolicy};
use enzui_services::process_transport::{ProcessRequest, StdinPolicy};
use image::RgbaImage;
use photos_library::raster;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VideoEdit {
    pub start: f32,
    pub end: f32,
    pub rotation: u8,
    pub flip: bool,
    pub mute: bool,
    pub speed: f32,
}

impl Default for VideoEdit {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 0.0,
            rotation: 0,
            flip: false,
            mute: false,
            speed: 1.0,
        }
    }
}

#[derive(Debug)]
pub(crate) struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration: f32,
}

fn run(
    path: &Path,
    args: Vec<OsString>,
    output_limit: usize,
    output_dir: Option<&Path>,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<Vec<u8>> {
    anyhow::ensure!(!cancelled(), "video operation cancelled");
    let input = raster::open_media(path)?;
    let identity = input.metadata()?;
    let inherited = std::fs::read_to_string("/proc/self/limits")?;
    let file_limit = inherited
        .lines()
        .find(|line| line.starts_with("Max file size"))
        .and_then(|line| line.split_whitespace().nth(4))
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(4 * 1024 * 1024 * 1024, |hard| {
            hard.min(4 * 1024 * 1024 * 1024)
        });
    let timeout = if output_dir.is_some() { 900 } else { 40 };
    let mut request = ProcessRequest::new(
        "/usr/bin/prlimit",
        StdinPolicy::Null,
        output_limit,
        32 * 1024,
        Duration::from_secs(timeout),
    )
    .arg("--as=1073741824")
    .arg(format!("--fsize={file_limit}"))
    .arg(format!("--cpu={timeout}"))
    .args([
        "--",
        "/usr/bin/bwrap",
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--ro-bind",
        "/usr",
        "/usr",
        "--symlink",
        "usr/lib",
        "/lib",
        "--symlink",
        "usr/lib",
        "/lib64",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--ro-bind",
    ])
    .arg(path)
    .arg("/input")
    .env("OMP_NUM_THREADS", "1");
    if let Some(output) = output_dir {
        request = request.arg("--bind").arg(output).arg("/output");
    }
    let cancel = CancelToken::new();
    let finished = AtomicBool::new(false);
    let output = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::Acquire) {
                if cancelled() {
                    cancel.cancel(CancelCause::Cancelled);
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        // The held descriptor prevents inode reuse; the in-namespace check rejects a swapped bind source.
        let request = request.args(["/usr/bin/sh", "-c", "test \"$(/usr/bin/stat -Lc '%d:%i' /input)\" = \"$1\" || exit 66; shift; exec \"$@\"", "photos-decoder"])
            .arg(format!("{}:{}", identity.dev(), identity.ino())).args(args);
        let result = enzui_services::process_transport::run(request, &cancel);
        finished.store(true, Ordering::Release);
        result
    })?;
    anyhow::ensure!(!cancelled(), "video operation cancelled");
    anyhow::ensure!(
        output.status.success(),
        "video decoder: {}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(240)
            .collect::<String>()
    );
    Ok(output.stdout)
}

pub(crate) fn probe(path: &Path, cancelled: &(impl Fn() -> bool + Sync)) -> Result<VideoInfo> {
    let args = [
        "/usr/bin/ffprobe",
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height:format=duration",
        "-of",
        "json",
        "/input",
    ]
    .map(OsString::from)
    .into();
    let bytes = run(path, args, 32 * 1024, None, cancelled)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let width = value["streams"][0]["width"]
        .as_u64()
        .context("missing video width")?;
    let height = value["streams"][0]["height"]
        .as_u64()
        .context("missing video height")?;
    anyhow::ensure!(
        width > 0
            && height > 0
            && width <= 32768
            && height <= 32768
            && width * height <= raster::MAX_PIXELS,
        "video exceeds the 64 megapixel frame limit"
    );
    let duration = value["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(0.0);
    Ok(VideoInfo {
        width: width as u32,
        height: height as u32,
        duration,
    })
}

pub(crate) fn frame(
    path: &Path,
    seconds: f32,
    limit: Option<u32>,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<RgbaImage> {
    probe(path, cancelled)?;
    let mut args: Vec<OsString> = [
        "/usr/bin/ffmpeg",
        "-nostdin",
        "-v",
        "error",
        "-threads",
        "1",
        "-protocol_whitelist",
        "file,pipe",
        "-ss",
    ]
    .map(OsString::from)
    .into();
    args.push(format!("{:.6}", seconds.max(0.0)).into());
    args.extend(["-i", "/input", "-an", "-sn", "-frames:v", "1"].map(OsString::from));
    if let Some(limit) = limit {
        args.extend([
            OsString::from("-vf"),
            format!("scale={limit}:{limit}:force_original_aspect_ratio=decrease").into(),
        ]);
    }
    args.extend(["-threads", "1", "-f", "image2pipe", "-c:v", "png", "pipe:1"].map(OsString::from));
    let bytes = run(
        path,
        args,
        raster::MAX_ENCODED_BYTES as usize,
        None,
        cancelled,
    )?;
    Ok(raster::decode(&bytes, cancelled)?)
}

pub(crate) fn extract_edited_frame(
    path: &Path,
    seconds: f32,
    edit: VideoEdit,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<PathBuf> {
    let pixels = frame(path, seconds, None, cancelled)?;
    let mut pixels = match edit.rotation % 4 {
        1 => image::imageops::rotate90(&pixels),
        2 => image::imageops::rotate180(&pixels),
        3 => image::imageops::rotate270(&pixels),
        _ => pixels,
    };
    if edit.flip {
        image::imageops::flip_horizontal_in_place(&mut pixels);
    }
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    for suffix in 1..=1000 {
        let output = path.with_file_name(format!(
            "{stem}-frame-{:.0}ms-{suffix}.png",
            seconds.max(0.0) * 1000.0
        ));
        match super::export_encoded(
            &pixels,
            &Default::default(),
            &output,
            Default::default(),
            None,
            cancelled,
        ) {
            Ok(path) => return Ok(path),
            Err(_) if output.try_exists()? => continue,
            Err(error) => return Err(error),
        }
    }
    anyhow::bail!("no unused frame filename is available")
}

pub(crate) fn copy_atomic(
    source: &Path,
    destination: &Path,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    let input = raster::open_media(source)?;
    let mut input = input.take(raster::MAX_MEDIA_BYTES + 1);
    let result = user_file::save_with(destination, SavePolicy::CreateNew, |output| {
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            if cancelled() {
                return Err(std::io::Error::other("media copy cancelled"));
            }
            let length = input.read(&mut buffer)?;
            if length == 0 {
                return Ok(());
            }
            total += length as u64;
            if total > raster::MAX_MEDIA_BYTES {
                return Err(std::io::Error::other("media exceeds 64 GiB"));
            }
            output.write_all(&buffer[..length])?;
        }
    });
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.was_published() => Err(anyhow::anyhow!(error)
            .context("media copy was published but its durability could not be confirmed")),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn export(
    path: &Path,
    destination: &Path,
    edit: VideoEdit,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<PathBuf> {
    anyhow::ensure!(
        destination
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("mp4")),
        "video copies use the MP4 extension"
    );
    anyhow::ensure!(
        !destination.try_exists()?,
        "choose a new filename; existing media is never overwritten"
    );
    let info = probe(path, cancelled)?;
    let end = if edit.end > 0.0 {
        edit.end.min(info.duration)
    } else {
        info.duration
    };
    anyhow::ensure!(
        edit.start.is_finite() && end.is_finite() && edit.start >= 0.0 && end - edit.start >= 0.05,
        "select a valid trim range"
    );
    anyhow::ensure!(
        edit.speed.is_finite() && (0.5..=2.0).contains(&edit.speed),
        "choose a speed between 0.5× and 2×"
    );
    let temporary = tempfile::tempdir()?;
    let mut args: Vec<OsString> = [
        "/usr/bin/ffmpeg",
        "-nostdin",
        "-v",
        "error",
        "-threads",
        "1",
        "-protocol_whitelist",
        "file,pipe",
        "-ss",
    ]
    .map(OsString::from)
    .into();
    args.push(edit.start.to_string().into());
    args.push("-t".into());
    args.push((end - edit.start).to_string().into());
    args.extend(["-i", "/input"].map(OsString::from));
    args.extend(["-map", "0:v:0"].map(OsString::from));
    if edit.mute {
        args.push("-an".into());
    } else {
        args.extend(["-map", "0:a?", "-c:a", "aac"].map(OsString::from));
        if edit.speed != 1.0 {
            args.extend([
                OsString::from("-af"),
                format!("atempo={}", edit.speed).into(),
            ]);
        }
    }
    let mut filters = Vec::new();
    match edit.rotation % 4 {
        1 => filters.push("transpose=1"),
        2 => filters.extend(["hflip", "vflip"]),
        3 => filters.push("transpose=2"),
        _ => {}
    }
    if edit.flip {
        filters.push("hflip");
    }
    filters.push("scale=trunc(iw/2)*2:trunc(ih/2)*2");
    let mut filters = filters.join(",");
    if edit.speed != 1.0 {
        filters.push_str(&format!(",setpts=(PTS-STARTPTS)/{}", edit.speed));
    }
    args.extend([OsString::from("-vf"), filters.into()]);
    args.extend(
        [
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "20",
            "-pix_fmt",
            "yuv420p",
            "-threads",
            "1",
            "-map_metadata",
            "-1",
            "-map_metadata:s",
            "-1",
            "-map_chapters",
            "-1",
            "-movflags",
            "+faststart",
            "/output/result.mp4",
        ]
        .map(OsString::from),
    );
    run(path, args, 1024, Some(temporary.path()), cancelled)?;
    copy_atomic(&temporary.path().join("result.mp4"), destination, cancelled)?;
    Ok(destination.to_owned())
}

pub(crate) fn filmstrip(
    path: &Path,
    duration: f32,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<Vec<iced::widget::image::Handle>> {
    (0..10)
        .map(|index| {
            let pixels = frame(path, duration * index as f32 / 10.0, Some(160), cancelled)?;
            Ok(iced::widget::image::Handle::from_rgba(
                pixels.width(),
                pixels.height(),
                pixels.into_raw(),
            ))
        })
        .collect()
}

pub(crate) fn transform_frame(
    source: &enzui_services::media::Rgba8Frame,
    edit: VideoEdit,
) -> Result<enzui_services::media::Rgba8Frame> {
    let pixels = RgbaImage::from_raw(source.width(), source.height(), source.pixels().to_vec())
        .context("invalid video frame")?;
    let mut pixels = match edit.rotation % 4 {
        1 => image::imageops::rotate90(&pixels),
        2 => image::imageops::rotate180(&pixels),
        3 => image::imageops::rotate270(&pixels),
        _ => pixels,
    };
    if edit.flip {
        image::imageops::flip_horizontal_in_place(&mut pixels);
    }
    Ok(enzui_services::media::Rgba8Frame::new(
        source.generation(),
        source.seq(),
        pixels.width(),
        pixels.height(),
        pixels.into_raw(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_atomic_copy_streams_beyond_photo_limit_and_cancellation_leaves_no_file() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let source = sandbox.path().join("large.mov");
        std::fs::File::create(&source)
            .expect("source")
            .set_len(raster::MAX_ENCODED_BYTES + 100)
            .expect("sparse");
        let destination = sandbox.path().join("copy.mov");
        let count = std::cell::Cell::new(0);
        assert!(
            copy_atomic(&source, &destination, &|| {
                count.set(count.get() + 1);
                count.get() > 3
            })
            .is_err()
        );
        assert!(!destination.exists());
        copy_atomic(&source, &destination, &|| false).expect("streamed copy");
        assert_eq!(
            std::fs::metadata(&destination).expect("copy length").len(),
            raster::MAX_ENCODED_BYTES + 100
        );
        assert!(copy_atomic(&source, &destination, &|| false).is_err());
    }
}
