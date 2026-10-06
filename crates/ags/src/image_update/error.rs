use std::fmt;

/// Failure of an image creation or update.
///
/// Every variant except [`ImageUpdateError::StateAfterPublish`] occurs before
/// publication: the configured image and committed state are unchanged.
#[derive(Debug)]
pub enum ImageUpdateError {
    Podman(String),
    Platform(String),
    Metadata {
        component: &'static str,
        message: String,
    },
    BaseUnavailable {
        reference: String,
        message: String,
    },
    Build {
        component: String,
        message: String,
    },
    Download {
        tool: String,
        message: String,
    },
    Verification(String),
    State(String),
    Lock(String),
    Assets(String),
    ExternalImageChange {
        image: String,
        found: String,
    },
    /// The verified image is published but its bookkeeping could not be
    /// recorded; the next run under the lock completes it.
    StateAfterPublish(String),
}

impl ImageUpdateError {
    pub(crate) fn podman(context: &str, message: impl fmt::Display) -> Self {
        Self::Podman(format!("{context}: {message}"))
    }

    pub(crate) fn build(component: impl Into<String>, message: impl fmt::Display) -> Self {
        Self::Build {
            component: component.into(),
            message: message.to_string(),
        }
    }

    /// Whether the configured image and committed state are known unchanged.
    pub fn preserved_existing_image(&self) -> bool {
        !matches!(self, Self::StateAfterPublish(_))
    }
}

impl fmt::Display for ImageUpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Podman(message) => write!(f, "podman failed: {message}"),
            Self::Platform(message) => write!(f, "unsupported platform: {message}"),
            Self::Metadata { component, message } => {
                write!(f, "{component} update check failed: {message}")
            }
            Self::BaseUnavailable { reference, message } => write!(
                f,
                "recorded Fedora base {reference} is unavailable ({message}); run `ags update-image --rebase` to adopt the current base of the same Fedora release"
            ),
            Self::Build { component, message } => {
                write!(f, "{component} build failed: {message}")
            }
            Self::Download { tool, message } => {
                write!(f, "verified download of {tool} failed: {message}")
            }
            Self::Verification(message) => {
                write!(f, "candidate image failed verification: {message}")
            }
            Self::State(message) => write!(f, "image update state: {message}"),
            Self::Lock(message) => write!(f, "image update lock: {message}"),
            Self::Assets(message) => write!(f, "could not prepare build recipes: {message}"),
            Self::ExternalImageChange { image, found } => write!(
                f,
                "{image} changed outside AGS while an update was interrupted (now {found}); AGS left it untouched and cleared its interrupted update record. Run `ags update-image` again to rebuild it"
            ),
            Self::StateAfterPublish(message) => write!(
                f,
                "the verified image was published but its update state could not be recorded ({message}); the next `ags update-image` completes the bookkeeping"
            ),
        }
    }
}

impl std::error::Error for ImageUpdateError {}
