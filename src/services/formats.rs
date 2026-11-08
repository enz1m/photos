use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use enzui_iced_core::cancellation::{CancelCause, CancelToken};
use enzui_services::process_transport::{ProcessRequest, StdinPolicy};
use iced::widget::image::Handle;
use image::RgbaImage;
use photos_library::{raster, scan};

const MOTION_WIDTH: u32 = 480;
const MOTION_HEIGHT: u32 = 360;
pub(crate) const MOTION_FPS: f32 = 15.0;
const MAX_FRAMES: usize = 90;

#[derive(Debug)]
pub(crate) struct Motion {
    pub path: PathBuf,
    pub frames: Vec<Handle>,
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub(crate) fn rendered_copy(path: &Path) -> bool {
    scan::is_camera_raw(path) || matches!(extension(path).as_str(), "psd" | "psb")
}

fn codec_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".local/lib/enz-photos/codecs"))
        .context("cannot locate the optional camera RAW decoder")
}

fn helper(
    bytes: &[u8],
    args: Vec<OsString>,
    output_limit: usize,
    cancelled: &(impl Fn() -> bool + Sync),
    raw: bool,
) -> Result<Vec<u8>> {
    anyhow::ensure!(!cancelled(), "decode cancelled");
    let temporary = tempfile::tempdir()?;
    let input = temporary.path().join("input");
    std::fs::write(&input, bytes)?;
    let policy = temporary.path().join("policy");
    std::fs::create_dir(&policy)?;
    std::fs::write(
        policy.join("policy.xml"),
        r#"<policymap>
<policy domain="delegate" rights="none" pattern="*"/>
<policy domain="filter" rights="none" pattern="*"/>
<policy domain="coder" rights="none" pattern="*"/>
<policy domain="coder" rights="read|write" pattern="{PNG,JPEG,WEBP,BMP,GIF,TIFF,ICO,TGA,PNM,QOI,HDR,EXR,HEIC,AVIF,JXL,PSD,PSB}"/>
<policy domain="resource" name="memory" value="256MiB"/>
<policy domain="resource" name="map" value="0"/>
<policy domain="resource" name="disk" value="0"/>
<policy domain="resource" name="thread" value="1"/>
<policy domain="resource" name="width" value="32768"/>
<policy domain="resource" name="height" value="32768"/>
<policy domain="resource" name="list-length" value="128"/>
</policymap>"#,
    )?;
    let mut request = ProcessRequest::new(
        "/usr/bin/prlimit",
        StdinPolicy::Null,
        output_limit,
        32 * 1024,
        Duration::from_secs(40),
    )
    .args([
        "--as=1073741824",
        "--cpu=35",
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
    .arg(input)
    .arg("/input")
    .arg("--ro-bind")
    .arg(policy)
    .arg("/policy")
    .env("MAGICK_CONFIGURE_PATH", "/policy")
    .env("OMP_NUM_THREADS", "1");
    if raw {
        let codecs = codec_dir()?;
        anyhow::ensure!(
            codecs.join("usr/bin/dcraw_emu").is_file(),
            "camera RAW viewing requires LibRaw (dcraw_emu)"
        );
        request = request
            .args(["--ro-bind"])
            .arg(codecs)
            .arg("/codecs")
            .env("LD_LIBRARY_PATH", "/codecs/usr/lib");
    }
    let request = request.args(args);
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
        let output = enzui_services::process_transport::run(request, &cancel);
        finished.store(true, Ordering::Release);
        output
    })?;
    anyhow::ensure!(
        output.status.success(),
        "decoder could not open this file: {}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(240)
            .collect::<String>()
    );
    anyhow::ensure!(!cancelled(), "decode cancelled");
    Ok(output.stdout)
}

pub(crate) fn decode(
    path: &Path,
    bytes: &[u8],
    limit: Option<u32>,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<RgbaImage> {
    if scan::is_camera_raw(path) {
        let mut args: Vec<OsString> = [
            "/codecs/usr/bin/dcraw_emu",
            "-w",
            "-q",
            "0",
            "-T",
            "-Z",
            "-",
        ]
        .map(OsString::from)
        .into();
        if limit.is_some() {
            args.push("-h".into());
        }
        args.push("/input".into());
        let output = helper(
            bytes,
            args,
            raster::MAX_ENCODED_BYTES as usize,
            cancelled,
            true,
        )?;
        return raster::decode(&output, cancelled).map_err(Into::into);
    }
    let limits = enzui_iced_core::decode_limits::DecodeLimits {
        max_encoded_bytes: raster::MAX_ENCODED_BYTES,
        max_width: 32768,
        max_height: 32768,
        max_pixels: raster::MAX_PIXELS,
        max_rgba_bytes: raster::MAX_PIXELS * 4,
    };
    let asset = if let Some(side) = limit {
        enzui_iced_resources::decode_static_resized(bytes, &limits, side, side, cancelled)
    } else {
        enzui_iced_resources::decode_static(bytes, &limits, cancelled)
    };
    if let Ok(asset) = asset {
        return super::pixels_from_handle(asset.handle);
    }
    anyhow::ensure!(!cancelled(), "decode cancelled");
    if let Ok(pixels) = raster::decode(bytes, cancelled) {
        return Ok(pixels);
    }
    let format = match extension(path).as_str() {
        "psd" => "PSD",
        "psb" => "PSB",
        "avif" => "AVIF",
        "heic" | "heif" | "heics" | "heifs" => "HEIC",
        "jxl" => "JXL",
        "tga" => "TGA",
        _ => anyhow::bail!("no decoder is available for this image"),
    };
    let input = format!("{format}:/input[0]");
    let dimensions = helper(
        bytes,
        [
            "/usr/bin/magick",
            "identify",
            "-ping",
            "-format",
            "%w %h",
            &input,
        ]
        .map(OsString::from)
        .into(),
        1024,
        cancelled,
        false,
    )?;
    let dimensions = String::from_utf8(dimensions)?;
    let mut values = dimensions
        .split_whitespace()
        .filter_map(|v| v.parse::<u32>().ok());
    let (w, h) = (
        values.next().context("missing image width")?,
        values.next().context("missing image height")?,
    );
    anyhow::ensure!(
        w > 0
            && h > 0
            && w <= 32768
            && h <= 32768
            && u64::from(w) * u64::from(h) <= raster::MAX_PIXELS,
        "image exceeds the 64 megapixel limit"
    );
    let mut args: Vec<OsString> = [
        "/usr/bin/magick",
        &input,
        "-auto-orient",
        "-colorspace",
        "sRGB",
        "-depth",
        "8",
    ]
    .map(OsString::from)
    .into();
    if let Some(limit) = limit {
        args.extend([
            OsString::from("-thumbnail"),
            format!("{limit}x{limit}>").into(),
        ]);
    }
    args.push("png:-".into());
    let output = helper(
        bytes,
        args,
        raster::MAX_ENCODED_BYTES as usize,
        cancelled,
        false,
    )?;
    raster::decode(&output, cancelled).map_err(Into::into)
}

fn sequence_brand(bytes: &[u8]) -> bool {
    if bytes.get(4..8) != Some(b"ftyp") {
        return false;
    }
    let length = bytes
        .get(..4)
        .and_then(|v| v.try_into().ok())
        .map(u32::from_be_bytes)
        .unwrap_or(0) as usize;
    bytes
        .get(8..length.min(bytes.len()).min(256))
        .is_some_and(|brands| {
            brands
                .as_chunks::<4>()
                .0
                .iter()
                .any(|brand| matches!(brand, b"msf1" | b"hevc" | b"hevx" | b"avis"))
        })
}

pub(crate) fn motion_companion(path: &Path) -> Option<PathBuf> {
    if !matches!(extension(path).as_str(), "heic" | "heif" | "jpg" | "jpeg") {
        return None;
    }
    ["mov", "MOV", "mp4", "MP4"]
        .into_iter()
        .map(|ext| path.with_extension(ext))
        .find(|p| {
            std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        })
}

pub(crate) fn motion(
    path: &Path,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<Option<Motion>> {
    let ext = extension(path);
    let companion = motion_companion(path);
    let heif_sequence = if companion.is_none() && matches!(ext.as_str(), "heic" | "heif") {
        sequence_brand(&raster::read_bounded(path)?)
    } else {
        false
    };
    let candidate = companion.or_else(|| {
        (heif_sequence
            || matches!(
                ext.as_str(),
                "gif" | "webp" | "png" | "apng" | "avif" | "heics" | "heifs"
            ))
        .then(|| path.to_owned())
    });
    let Some(path) = candidate else {
        return Ok(None);
    };
    let bytes = raster::read_bounded(&path)?;
    let args = ["/usr/bin/ffmpeg", "-nostdin", "-v", "error", "-threads", "1", "-protocol_whitelist", "file,pipe", "-i", "/input", "-an", "-sn", "-vf",
        "fps=15,scale=480:360:force_original_aspect_ratio=decrease,pad=480:360:(ow-iw)/2:(oh-ih)/2:color=black@0,format=rgba",
        "-frames:v", "90", "-threads", "1", "-f", "rawvideo", "pipe:1"].map(OsString::from).into();
    let frame_bytes = (MOTION_WIDTH * MOTION_HEIGHT * 4) as usize;
    let output = helper(&bytes, args, MAX_FRAMES * frame_bytes, cancelled, false)?;
    anyhow::ensure!(
        output.len() % frame_bytes == 0,
        "motion decoder returned an incomplete frame"
    );
    let frames: Vec<_> = output
        .chunks_exact(frame_bytes)
        .map(|pixels| Handle::from_rgba(MOTION_WIDTH, MOTION_HEIGHT, pixels.to_vec()))
        .collect();
    Ok((frames.len() > 1).then(|| Motion { path, frames }))
}

fn motion_dimensions(bytes: &[u8], cancelled: &(impl Fn() -> bool + Sync)) -> Result<(u32, u32)> {
    let args = [
        "/usr/bin/ffprobe",
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height",
        "-of",
        "json",
        "/input",
    ]
    .map(OsString::from)
    .into();
    let output = helper(bytes, args, 16 * 1024, cancelled, false)?;
    let probe: serde_json::Value = serde_json::from_slice(&output)?;
    let stream = &probe["streams"][0];
    let width = stream["width"].as_u64().context("missing frame width")?;
    let height = stream["height"].as_u64().context("missing frame height")?;
    anyhow::ensure!(
        width > 0
            && height > 0
            && width <= 32768
            && height <= 32768
            && width.saturating_mul(height) <= raster::MAX_PIXELS,
        "frame exceeds the 64 megapixel limit"
    );
    Ok((u32::try_from(width)?, u32::try_from(height)?))
}

pub(crate) fn extract_frame(
    path: &Path,
    frame: usize,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<PathBuf> {
    let bytes = raster::read_bounded(path)?;
    motion_dimensions(&bytes, cancelled)?;
    let seconds = format!("{:.6}", frame as f32 / MOTION_FPS);
    let args = [
        "/usr/bin/ffmpeg",
        "-nostdin",
        "-v",
        "error",
        "-threads",
        "1",
        "-protocol_whitelist",
        "file,pipe",
        "-i",
        "/input",
        "-ss",
        &seconds,
        "-frames:v",
        "1",
        "-threads",
        "1",
        "-f",
        "image2pipe",
        "-c:v",
        "png",
        "pipe:1",
    ]
    .map(OsString::from)
    .into();
    let output = helper(
        &bytes,
        args,
        raster::MAX_ENCODED_BYTES as usize,
        cancelled,
        false,
    )?;
    let pixels = raster::decode(&output, cancelled)?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    for suffix in 1..=1000 {
        let target = path.with_file_name(format!("{stem}-frame-{}-{suffix}.png", frame + 1));
        match super::export_encoded(
            &pixels,
            &Default::default(),
            &target,
            super::ExportOptions::default(),
            None,
            cancelled,
        ) {
            Ok(path) => return Ok(path),
            Err(_) if target.exists() => continue,
            Err(error) => return Err(error),
        }
    }
    anyhow::bail!("no unused frame filename is available")
}

pub(crate) fn may_animate(path: &Path, bytes: &[u8]) -> bool {
    match extension(path).as_str() {
        "gif" | "heics" | "heifs" => true,
        "heic" | "heif" | "avif" => sequence_brand(bytes),
        "png" | "apng" => bytes.windows(4).any(|v| v == b"acTL"),
        "webp" => bytes.windows(4).any(|v| v == b"ANIM"),
        _ => false,
    }
}
