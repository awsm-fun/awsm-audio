//! Loading a saved editor **project** document into a bare [`SampleLibrary`].
//!
//! The editor exports a project as a top-level document that *wraps* the library
//! alongside editor-only view-state (camera pan/zoom, per-node layout):
//!
//! ```toml
//! pan_x = 0.0
//! pan_y = 0.0
//! zoom = 1.0
//!
//! [library]
//! version = 1
//! # … samples, assets, listener …
//! ```
//!
//! A player only needs the `[library]` part. Without this module a consumer has
//! to hand-roll a `#[derive(Deserialize)] struct { library: SampleLibrary }`
//! wrapper (and pull in a TOML crate) just to peel that layer off. [`Project`]
//! is that wrapper, and [`from_toml_str`] / [`library_from_toml`] (behind the
//! `toml` feature) do the peeling — accepting either the wrapped project form or
//! a bare [`SampleLibrary`].

use serde::Deserialize;

use crate::library::SampleLibrary;

/// The portable subset of a saved project document: the [`SampleLibrary`] the
/// player consumes. The editor's `project.toml` carries extra view-state fields
/// (`pan_x`/`pan_y`/`zoom`/`layout`) next to `[library]`; those are editor-only
/// and ignored here.
#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    /// The audio document.
    pub library: SampleLibrary,
}

impl Project {
    /// Consume the project, yielding just its [`SampleLibrary`].
    pub fn into_library(self) -> SampleLibrary {
        self.library
    }
}

/// Parse a TOML string that is **either** a `[library]`-wrapped project document
/// **or** a bare [`SampleLibrary`], returning the [`SampleLibrary`] either way.
///
/// The two forms are disambiguated structurally (presence of a top-level
/// `library` table), so a malformed wrapped document reports its real parse error
/// rather than silently falling back to an empty library.
#[cfg(feature = "toml")]
pub fn from_toml_str(s: &str) -> Result<SampleLibrary, toml::de::Error> {
    let value: toml::Value = toml::from_str(s)?;
    if value.get("library").is_some() {
        Project::deserialize(value).map(Project::into_library)
    } else {
        SampleLibrary::deserialize(value)
    }
}

/// Alias of [`from_toml_str`], named to read well at the call site
/// (`library_from_toml(&text)?`).
#[cfg(feature = "toml")]
pub fn library_from_toml(s: &str) -> Result<SampleLibrary, toml::de::Error> {
    from_toml_str(s)
}

#[cfg(all(test, feature = "toml"))]
mod tests {
    use super::*;
    use crate::library::Listener;
    use serde::Serialize;

    /// Stand-in for the editor's on-disk project: the library plus view-state.
    #[derive(Serialize)]
    struct Wrapped {
        pan_x: f64,
        pan_y: f64,
        zoom: f64,
        library: SampleLibrary,
    }

    fn sample_library() -> SampleLibrary {
        // A non-default field so we can tell a correct extraction from a silent
        // empty-library fallback.
        SampleLibrary {
            listener: Some(Listener::default()),
            ..SampleLibrary::default()
        }
    }

    #[test]
    fn loads_wrapped_project_and_ignores_view_state() {
        let lib = sample_library();
        let wrapped = Wrapped {
            pan_x: 12.0,
            pan_y: -3.0,
            zoom: 2.5,
            library: lib.clone(),
        };
        let toml_str = toml::to_string(&wrapped).unwrap();
        let loaded = from_toml_str(&toml_str).unwrap();
        assert_eq!(loaded, lib);
        assert!(loaded.listener.is_some());
    }

    #[test]
    fn loads_bare_library() {
        let lib = sample_library();
        let toml_str = toml::to_string(&lib).unwrap();
        let loaded = from_toml_str(&toml_str).unwrap();
        assert_eq!(loaded, lib);
    }
}
