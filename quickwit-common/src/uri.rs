// Copyright (C) 2021 Quickwit, Inc.
//
// Quickwit is offered under the AGPL v3.0 and as commercial software.
// For commercial licensing, contact us at hello@quickwit.io.
//
// AGPL:
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.

use std::env;
use std::ffi::OsStr;
use std::fmt::Display;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context};

/// Default file protocol `file://`
const FILE_PROTOCOL: &str = "file";

const PROTOCOL_SEPARATOR: &str = "://";

#[derive(Debug, PartialEq)]
pub enum Extension {
    Json,
    Toml,
    Unknown(String),
    Yaml,
}

impl Extension {
    fn maybe_new(extension: &str) -> Option<Self> {
        match extension {
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            "" => None,
            unknown => Some(Self::Unknown(unknown.to_string())),
        }
    }
}

/// Encapsulates the URI type.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Uri {
    uri: String,
    protocol_idx: usize,
}

impl Uri {
    /// Attempts to construct a [`Uri`] from a raw string slice.
    /// A `file://` protocol is assumed if not specified.
    /// File URIs are resolved (normalized) relative to the current working directory
    /// unless an absolute path is specified.
    /// Handles special characters like `~`, `.`, `..`.
    pub fn try_new(uri: &str) -> anyhow::Result<Self> {
        if uri.is_empty() {
            bail!("URI is empty.");
        }
        let (protocol, mut path) = match uri.split_once(PROTOCOL_SEPARATOR) {
            None => (FILE_PROTOCOL, uri.to_string()),
            Some((protocol, path)) => (protocol, path.to_string()),
        };
        if protocol == FILE_PROTOCOL {
            if path.starts_with('~') {
                // We only accept `~` (alias to the home directory) and `~/path/to/something`.
                // If there is something following the `~` that is not `/`, we bail out.
                if path.len() > 1 && !path.starts_with("~/") {
                    bail!("Path syntax `{}` is not supported.", uri);
                }

                let home_dir_path = home::home_dir()
                    .context("Failed to resolve home directory.")?
                    .to_string_lossy()
                    .to_string();

                path.replace_range(0..1, &home_dir_path);
            }
            if !path.starts_with('/') {
                let current_dir = env::current_dir().context(
                    "Failed to resolve current working directory: dir does not exist or \
                     insufficient permissions.",
                )?;
                path = current_dir.join(path).to_string_lossy().to_string();
            }
            path = normalize_path(Path::new(&path))
                .to_string_lossy()
                .to_string();
        }
        Ok(Self {
            uri: format!("{}{}{}", protocol, PROTOCOL_SEPARATOR, path),
            protocol_idx: protocol.len(),
        })
    }

    /// Returns the extension of the URI.
    pub fn extension(&self) -> Option<Extension> {
        Path::new(&self.uri)
            .extension()
            .and_then(OsStr::to_str)
            .and_then(Extension::maybe_new)
    }

    /// Returns the protocol of the URI.
    pub fn protocol(&self) -> &str {
        &self.uri[..self.protocol_idx]
    }

    /// Returns the file path of the URI.
    /// Applies only to `file://` URIs.
    pub fn filepath(&self) -> Option<&Path> {
        if self.protocol() == "file" {
            self.uri.strip_prefix("file://").map(Path::new)
        } else {
            None
        }
    }

    /// Consumes the [`Uri`] struct and returns the normalized URI as a string.
    pub fn into_string(self) -> String {
        self.uri
    }
}

impl AsRef<str> for Uri {
    fn as_ref(&self) -> &str {
        &self.uri
    }
}

impl Display for Uri {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.uri)
    }
}

impl PartialEq<&str> for Uri {
    fn eq(&self, other: &&str) -> bool {
        &self.uri == other
    }
}
impl PartialEq<String> for Uri {
    fn eq(&self, other: &String) -> bool {
        &self.uri == other
    }
}

/// Normalizes a path by resolving the components like (., ..).
/// This helper does the same thing as `Path::canonicalize`.
/// It only differs from `Path::canonicalize` by not checking file existence
/// during resolution.
/// <https://github.com/rust-lang/cargo/blob/fede83ccf973457de319ba6fa0e36ead454d2e20/src/cargo/util/paths.rs#L61>
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut resulting_path_buf =
        if let Some(component @ Component::Prefix(..)) = components.peek().cloned() {
            components.next();
            PathBuf::from(component.as_os_str())
        } else {
            PathBuf::new()
        };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                resulting_path_buf.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                resulting_path_buf.pop();
            }
            Component::Normal(inner_component) => {
                resulting_path_buf.push(inner_component);
            }
        }
    }
    resulting_path_buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_new_uri() {
        Uri::try_new("").unwrap_err();

        let home_dir = home::home_dir().unwrap();
        let current_dir = env::current_dir().unwrap();

        let uri = Uri::try_new("file:///home/foo/bar").unwrap();
        assert_eq!(uri.protocol(), "file");
        assert_eq!(uri.filepath(), Some(Path::new("/home/foo/bar")));
        assert_eq!(uri.as_ref(), "file:///home/foo/bar");
        assert_eq!(uri, "file:///home/foo/bar");
        assert_eq!(uri, "file:///home/foo/bar".to_string());

        assert_eq!(
            Uri::try_new("home/homer/docs/dognuts").unwrap(),
            format!("file://{}/home/homer/docs/dognuts", current_dir.display())
        );
        assert_eq!(
            Uri::try_new("home/homer/docs/../dognuts").unwrap(),
            format!("file://{}/home/homer/dognuts", current_dir.display())
        );
        assert_eq!(
            Uri::try_new("home/homer/docs/../../dognuts").unwrap(),
            format!("file://{}/home/dognuts", current_dir.display())
        );
        assert_eq!(
            Uri::try_new("/home/homer/docs/dognuts").unwrap(),
            "file:///home/homer/docs/dognuts"
        );
        assert_eq!(
            Uri::try_new("~").unwrap(),
            format!("file://{}", home_dir.display())
        );
        assert_eq!(
            Uri::try_new("~/").unwrap(),
            format!("file://{}", home_dir.display())
        );
        assert_eq!(
            Uri::try_new("~anything/bar").unwrap_err().to_string(),
            "Path syntax `~anything/bar` is not supported."
        );
        assert_eq!(
            Uri::try_new("~/.").unwrap(),
            format!("file://{}", home_dir.display())
        );
        assert_eq!(
            Uri::try_new("~/..").unwrap(),
            format!("file://{}", home_dir.parent().unwrap().display())
        );
        assert_eq!(
            Uri::try_new("file://").unwrap(),
            format!("file://{}", current_dir.display())
        );
        assert_eq!(
            Uri::try_new("file://.").unwrap(),
            format!("file://{}", current_dir.display())
        );
        assert_eq!(
            Uri::try_new("file://..").unwrap(),
            format!("file://{}", current_dir.parent().unwrap().display())
        );
        assert_eq!(
            Uri::try_new("s3://home/homer/docs/dognuts").unwrap(),
            "s3://home/homer/docs/dognuts"
        );
        assert_eq!(
            Uri::try_new("s3://home/homer/docs/../dognuts").unwrap(),
            "s3://home/homer/docs/../dognuts"
        );
    }

    #[test]
    fn test_uri_extension() {
        assert!(Uri::try_new("s3://").unwrap().extension().is_none());

        assert_eq!(
            Uri::try_new("s3://config.json")
                .unwrap()
                .extension()
                .unwrap(),
            Extension::Json
        );
        assert_eq!(
            Uri::try_new("s3://config.foo")
                .unwrap()
                .extension()
                .unwrap(),
            Extension::Unknown("foo".to_string())
        );
    }
}
