# Synthetic decode fixture

`synthetic.heic` is an original 64 × 64 RGB gradient: pixel `(x, y)` is
`(4x, 4y, 96)`. It contains no personal photo or location data. It was encoded
with libheif 1.23.1 using:

```sh
heif-enc -q 50 -p x265:pools=1 -p x265:frame-threads=1 \
  -o synthetic.heic gradient.png
```

The service workflow stores these bytes under a JPEG filename, decodes by
content, edits and exports a new PNG, and verifies that the source is unchanged.
The fixture is part of Photos under the repository's MIT license; its encoder
is not a dependency of the test.

`solid-960x640.heic` is a synthetic solid-color 960 × 640 image used to verify
that oversized HEIC decoder output is reduced before thumbnail-cache admission.
It contains no personal media or metadata and is covered by the project license.
