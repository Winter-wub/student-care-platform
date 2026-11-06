//! Build-time route checks, so mounting the Router can never panic: every
//! path is valid for axum and no two routes match the same requests.

use crate::error::BuildError;

/// Paths the Kernel mounts itself in `Deployment::router`.
pub(super) const WEBHOOK_PATH: &str = "/webhook/{kind}";

/// Prefix of every back-office path.
pub(super) const ADMIN: &str = "/admin";

/// The paths registered so far: (normalised path, owning Module).
pub(super) struct Routes(Vec<(String, &'static str)>);

impl Routes {
    /// Starts with the Kernel's own paths.
    pub(super) fn new() -> Self {
        let kernel = crate::kernel::NAME;
        Self(vec![
            (normalise(WEBHOOK_PATH), kernel),
            (normalise(crate::ui::CSS_PATH), kernel),
        ])
    }

    /// A public route at `path`.
    pub(super) fn public(&mut self, module: &'static str, path: &str) -> Result<(), BuildError> {
        if path == ADMIN || path.starts_with("/admin/") {
            return Err(invalid(module, path));
        }
        self.add(module, path.to_string(), path)
    }

    /// A back-office route at `/admin{path}`.
    pub(super) fn back_office(
        &mut self,
        module: &'static str,
        path: &str,
    ) -> Result<(), BuildError> {
        self.add(module, format!("{ADMIN}{path}"), path)
    }

    fn add(
        &mut self,
        module: &'static str,
        full: String,
        as_given: &str,
    ) -> Result<(), BuildError> {
        if !as_given.starts_with('/')
            || full
                .split('/')
                .any(|seg| seg.starts_with(':') || seg.starts_with('*'))
        {
            return Err(invalid(module, as_given));
        }
        let key = normalise(&full);
        if let Some((_, first)) = self.0.iter().find(|(k, _)| *k == key) {
            return Err(BuildError::DuplicateRoute {
                module,
                path: full,
                first,
            });
        }
        self.0.push((key, module));
        Ok(())
    }
}

fn invalid(module: &'static str, path: &str) -> BuildError {
    BuildError::InvalidRoute {
        module,
        path: path.to_string(),
    }
}

/// `{name}` and `{*name}` segments compared without their names: axum
/// refuses `/a/{x}` next to `/a/{y}` (and a catch-all next to a param).
fn normalise(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.starts_with('{') && seg.ends_with('}') {
                "{}"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_are_compared_without_names() {
        let mut r = Routes::new();
        r.public("a", "/news/{id}").unwrap();
        assert!(matches!(
            r.public("b", "/news/{slug}"),
            Err(BuildError::DuplicateRoute { first: "a", .. })
        ));
        r.public("b", "/news/latest").unwrap();
        assert!(matches!(
            r.public("b", "/webhook/{channel}"),
            Err(BuildError::DuplicateRoute {
                first: "kernel",
                ..
            })
        ));
    }

    #[test]
    fn invalid_paths_are_refused() {
        let mut r = Routes::new();
        for p in ["news", "/news/:id", "/files/*rest", "/admin", "/admin/x"] {
            assert!(
                matches!(r.public("a", p), Err(BuildError::InvalidRoute { .. })),
                "{p}"
            );
        }
        r.back_office("a", "/reports").unwrap();
        r.public("a", "/reports").unwrap();
        assert!(r.back_office("b", "/reports").is_err());
    }
}
