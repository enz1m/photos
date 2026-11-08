# Media support

Photos reads JPEG, PNG/APNG, WebP, BMP, GIF, TIFF, ICO, TGA, PNM, QOI, HDR and
OpenEXR through Rust image decoders. HEIC/HEIF stills use the framework's libheif
support and an installed decoder plugin. Optional host decoders add AVIF, JPEG
XL, Photoshop PSD/PSB composites and camera RAW formats. Discovery by extension
does not guarantee that every codec variant or camera model can decode.

ImageMagick handles AVIF, JPEG XL and Photoshop composites, including conversion
to sRGB. RAW development uses LibRaw's `dcraw_emu`. Configure its private runtime
under `~/.local/lib/enz-photos/codecs/usr/` with the executable in `bin` and shared
libraries in `lib`. The app does not install codecs or replace system packages.
Helpers require bubblewrap and util-linux `prlimit`; media helpers use FFmpeg.

## Viewing and editing

Edits are reversible catalog recipes. Crop, rotate, flip, resize and color
adjustments leave originals unchanged. Export produces a new flattened PNG,
JPEG or WebP and refuses to overwrite an existing file. Metadata is cleared by
default. Retaining metadata preserves supported EXIF/GPS fields and writes an
sRGB profile for normalized RGB pixels. PSD layers and RAW sensor data are not
written back. HDR sources currently display through an SDR pipeline.

## Video and motion photos

A same-stem MOV or MP4 beside a JPEG/HEIC/HEIF still is treated as its motion
companion. Filename pairing does not validate Apple's asset identifier. Keep
pairs together; importing a pair preserves matching names when a collision needs
a suffix.

Standalone videos and motion companions use GStreamer. Installed plugins decide
codec support; base/good/bad/ugly plugins and gst-libav provide broad coverage.
Playback supports seeking, frame stepping and source-resolution frame saving.
Video editing provides trim, rotation, flip, mute and playback-speed controls;
export creates a new file. Draft video edits are not persisted across restarts.
Animated raster and HEIF-sequence playback uses a bounded, silent FFmpeg fallback;
sequence support depends on the installed container and codec support.

## Resource and file safety

Still-image admission is limited to 128 MiB encoded input and 64 megapixels for
editing. Viewer work is separate from gallery thumbnails. Originals are bound
read-only to isolated helpers; cancellation and retired generations cannot
replace a newer viewer or publish partial exports. Original files remain
unchanged even when decoding or export fails.
