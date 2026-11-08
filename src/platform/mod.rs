use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct Paths {
    pub photos: PathBuf,
    pub catalog: PathBuf,
}

impl Paths {
    pub(crate) fn thumbnail_cache(&self) -> PathBuf {
        let standard = dirs::state_dir().map(|dir| dir.join("enz-photos/catalog.json"));
        if standard.as_ref() == Some(&self.catalog) {
            dirs::cache_dir()
                .unwrap_or_else(|| {
                    self.catalog
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_owned()
                })
                .join("enz-photos/thumbnails-v1")
        } else {
            self.catalog
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("thumbnail-cache-v1")
        }
    }
    pub fn discover() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let photos = std::env::var_os("ENZ_PHOTOS_LIBRARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("Photos"));
        let state = std::env::var_os("ENZ_PHOTOS_STATE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::state_dir()
                    .unwrap_or_else(|| home.join(".local/state"))
                    .join("enz-photos/catalog.json")
            });
        Self {
            photos,
            catalog: state,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Launch {
    pub file: Option<PathBuf>,
    pub add_folder: Option<PathBuf>,
    pub measure: Option<PathBuf>,
    pub help: bool,
    pub started: std::time::Instant,
}

impl Default for Launch {
    fn default() -> Self {
        Self {
            file: None,
            add_folder: None,
            measure: None,
            help: false,
            started: std::time::Instant::now(),
        }
    }
}

impl Launch {
    pub(crate) fn parse(
        arguments: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<Self, String> {
        let mut launch = Self::default();
        let mut arguments = arguments.into_iter();
        let mut positional = false;
        while let Some(argument) = arguments.next() {
            if !positional && argument == "--" {
                positional = true;
                continue;
            }
            if !positional && (argument == "--help" || argument == "-h") {
                launch.help = true;
                continue;
            }
            let destination = if !positional && argument == "--add-folder" {
                &mut launch.add_folder
            } else if !positional && argument == "--measure-thumbnails" {
                &mut launch.measure
            } else if !positional && argument == "--open" {
                &mut launch.file
            } else {
                if !positional && argument.as_encoded_bytes().starts_with(b"-") {
                    return Err(format!("unknown option: {}", argument.to_string_lossy()));
                }
                if launch.file.is_some() {
                    return Err("open one media file at a time".into());
                }
                launch.file = Some(local_path(argument)?);
                continue;
            };
            if destination.is_some() {
                return Err("option specified more than once".into());
            }
            let value = arguments
                .next()
                .ok_or_else(|| format!("{} requires a path", argument.to_string_lossy()))?;
            *destination = Some(local_path(value)?);
        }
        if launch.measure.is_some() && (launch.file.is_some() || launch.add_folder.is_some()) {
            return Err(
                "thumbnail measurement cannot be combined with opening media or adding folders"
                    .into(),
            );
        }
        Ok(launch)
    }
}

fn local_path(value: std::ffi::OsString) -> Result<PathBuf, String> {
    if let Some(text) = value.to_str() {
        if text.starts_with("file:") {
            return enzui_iced_core::portal::uri::file_uri_to_path(text).map_err(|e| e.to_string());
        }
        if text.contains("://") {
            return Err("only local media files can be opened".into());
        }
    }
    if value.is_empty() {
        return Err("a media path cannot be empty".into());
    }
    Ok(value.into())
}

#[cfg(test)]
mod direct_open_tests {
    use super::*;
    #[test]
    fn direct_open_arguments_preserve_local_paths_and_reject_remote_or_multiple_files() {
        let parse = |args: &[&str]| Launch::parse(args.iter().map(std::ffi::OsString::from));
        assert_eq!(
            parse(&["file:///tmp/hello%20world%2B.jpg"])
                .expect("valid test fixture")
                .file,
            Some("/tmp/hello world+.jpg".into())
        );
        assert_eq!(
            parse(&["--open", "relative photo.png"])
                .expect("valid test fixture")
                .file,
            Some("relative photo.png".into())
        );
        assert_eq!(
            parse(&["--", "-photo.png"])
                .expect("valid test fixture")
                .file,
            Some("-photo.png".into())
        );
        assert!(parse(&["file://other-host/tmp/a.png"]).is_err());
        assert!(parse(&["https://example.com/a.png"]).is_err());
        assert!(parse(&["a.png", "b.png"]).is_err());
        assert!(parse(&["--open"]).is_err());
        use std::os::unix::ffi::OsStringExt;
        let bytes = std::ffi::OsString::from_vec(b"/tmp/photo-\xff.png".to_vec());
        assert_eq!(
            Launch::parse([bytes.clone()])
                .expect("valid test fixture")
                .file,
            Some(bytes.into())
        );
    }
}
