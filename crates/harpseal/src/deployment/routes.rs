//! Build-time route checks, so mounting the Router can never panic: every
//! path is one axum 0.8 accepts and no two routes conflict.
//!
//! Rules (stricter than axum where that is simpler to state):
//! - a path starts with `/`;
//! - a segment holding `{` or `}` is exactly `{name}` or `{*name}`, with
//!   `name` made of ASCII letters, digits and `_` (no `/x{y}`, `{}`,
//!   `{a}{b}`, unbalanced braces, or the old `:name` / `*name` syntax);
//! - a catch-all `{*name}` is only the last segment;
//! - two paths equal once parameter names are ignored conflict;
//! - a catch-all at segment `i` conflicts with any path that has a
//!   parameter or catch-all at `i` after the same prefix;
//! - a public path is not under `/admin` and its first segment is not a
//!   parameter (it would answer every unregistered `/admin/...` path).

use crate::error::BuildError;

/// Paths the Kernel mounts itself in `Deployment::router`.
pub(super) const WEBHOOK_PATH: &str = "/webhook/{kind}";

/// Prefix of every back-office path.
pub(super) const ADMIN: &str = "/admin";

/// One segment, parameter names dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Static(String),
    Param,
    CatchAll,
}

impl Seg {
    fn is_param(&self) -> bool {
        matches!(self, Seg::Param | Seg::CatchAll)
    }
}

/// The paths registered so far: (segments, owning Module).
pub(super) struct Routes(Vec<(Vec<Seg>, &'static str)>);

impl Routes {
    /// Starts with the Kernel's own paths: webhooks, the stylesheet and the
    /// back-office home, sign-in, sign-out and password pages.
    pub(super) fn new() -> Self {
        let kernel = crate::kernel::NAME;
        let own = [
            WEBHOOK_PATH,
            crate::ui::CSS_PATH,
            crate::auth::HOME_PATH,
            crate::auth::LOGIN_PATH,
            crate::auth::LOGOUT_PATH,
            crate::auth::PASSWORD_PATH,
        ];
        Self(
            own.iter()
                .map(|p| (parse(p).expect("Kernel paths are valid"), kernel))
                .collect(),
        )
    }

    /// A public route at `path`.
    pub(super) fn public(&mut self, module: &'static str, path: &str) -> Result<(), BuildError> {
        let segs = parse(path).ok_or_else(|| invalid(module, path))?;
        let under_admin = segs.first() == Some(&Seg::Static("admin".into()));
        let open_first = segs.first().is_some_and(Seg::is_param);
        if under_admin || open_first {
            return Err(invalid(module, path));
        }
        self.add(module, segs, path.to_string())
    }

    /// A back-office route at `/admin{path}`.
    pub(super) fn back_office(
        &mut self,
        module: &'static str,
        path: &str,
    ) -> Result<(), BuildError> {
        parse(path).ok_or_else(|| invalid(module, path))?;
        let full = format!("{ADMIN}{path}");
        let segs = parse(&full).ok_or_else(|| invalid(module, path))?;
        self.add(module, segs, full)
    }

    fn add(
        &mut self,
        module: &'static str,
        segs: Vec<Seg>,
        full: String,
    ) -> Result<(), BuildError> {
        if let Some((_, first)) = self.0.iter().find(|(other, _)| conflict(&segs, other)) {
            return Err(BuildError::DuplicateRoute {
                module,
                path: full,
                first,
            });
        }
        self.0.push((segs, module));
        Ok(())
    }
}

fn invalid(module: &'static str, path: &str) -> BuildError {
    BuildError::InvalidRoute {
        module,
        path: path.to_string(),
    }
}

/// The segments of a valid path, `None` when axum could refuse it.
fn parse(path: &str) -> Option<Vec<Seg>> {
    let rest = path.strip_prefix('/')?;
    let raw: Vec<&str> = rest.split('/').collect();
    let mut segs = Vec::with_capacity(raw.len());
    for (i, s) in raw.iter().enumerate() {
        let seg = if s.starts_with(':') || s.starts_with('*') {
            return None;
        } else if let Some(inner) = s.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
            let (name, seg) = match inner.strip_prefix('*') {
                Some(name) => (name, Seg::CatchAll),
                None => (inner, Seg::Param),
            };
            let ident =
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            if !ident || (seg == Seg::CatchAll && i + 1 != raw.len()) {
                return None;
            }
            seg
        } else if s.contains('{') || s.contains('}') {
            return None;
        } else {
            Seg::Static(s.to_string())
        };
        segs.push(seg);
    }
    Some(segs)
}

/// Whether axum would refuse `a` and `b` side by side.
fn conflict(a: &[Seg], b: &[Seg]) -> bool {
    a == b || catch_all_clash(a, b) || catch_all_clash(b, a)
}

/// `a` ends in a catch-all at `i`; `b` has a parameter there after the
/// same prefix.
fn catch_all_clash(a: &[Seg], b: &[Seg]) -> bool {
    let Some(i) = a.iter().position(|s| *s == Seg::CatchAll) else {
        return false;
    };
    b.len() > i && a[..i] == b[..i] && b[i].is_param()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mounts `paths` on an axum Router: true when axum panics.
    fn axum_panics(paths: &[&str]) -> bool {
        let paths: Vec<String> = paths.iter().map(|s| s.to_string()).collect();
        std::panic::catch_unwind(move || {
            let mut r: axum::Router = axum::Router::new();
            for p in &paths {
                r = r.route(p, axum::routing::get(|| async { "" }));
            }
        })
        .is_err()
    }

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
        let bad = [
            "news",
            "/news/:id",
            "/files/*rest",
            "/admin",
            "/admin/x",
            "/a/{*rest}/b",
            "/a/{id",
            "/a/}",
            "/a/{}",
            "/a/{*}",
            "/a/{x}{y}",
            "/a/x{y}",
            "/a/{x-y}",
        ];
        for p in bad {
            assert!(
                matches!(r.public("a", p), Err(BuildError::InvalidRoute { .. })),
                "{p}"
            );
        }
        r.back_office("a", "/reports").unwrap();
        r.public("a", "/reports").unwrap();
        for own in ["/login", "/logout", "/password"] {
            assert!(matches!(
                r.back_office("b", own),
                Err(BuildError::DuplicateRoute {
                    first: "kernel",
                    ..
                })
            ));
        }
        assert!(r.back_office("b", "/reports").is_err());
        assert!(r.back_office("b", "/a/{*rest}/b").is_err());
    }

    #[test]
    fn public_paths_may_not_start_with_a_parameter() {
        let mut r = Routes::new();
        for p in ["/{page}", "/{*rest}", "/{lang}/news"] {
            assert!(
                matches!(r.public("a", p), Err(BuildError::InvalidRoute { .. })),
                "{p}"
            );
        }
        r.back_office("a", "/{page}").unwrap();
    }

    #[test]
    fn a_catch_all_clashes_with_parameters_after_the_same_prefix() {
        let clashes: [(&str, &str); 4] = [
            ("/files/{*rest}", "/files/{id}/x"),
            ("/files/{*rest}", "/files/{id}"),
            ("/files/{id}/x", "/files/{*rest}"),
            ("/files/{*a}", "/files/{*b}"),
        ];
        for (a, b) in clashes {
            assert!(axum_panics(&[a, b]), "axum refuses {a} + {b}");
            let mut r = Routes::new();
            r.public("a", a).unwrap();
            assert!(
                matches!(r.public("b", b), Err(BuildError::DuplicateRoute { .. })),
                "{a} + {b}"
            );
        }
        let fine = [
            ("/files/{*rest}", "/files/x"),
            ("/files/{*rest}", "/files"),
            ("/files/{*rest}", "/docs/{id}"),
            ("/a/{x}/b", "/a/{y}/c"),
        ];
        for (a, b) in fine {
            assert!(!axum_panics(&[a, b]), "axum accepts {a} + {b}");
            let mut r = Routes::new();
            r.public("a", a).unwrap();
            r.public("b", b).unwrap();
        }
    }

    #[test]
    fn every_refused_single_path_really_panics_in_axum() {
        for p in ["/a/{*rest}/b", "/a/{id", "/a/{}", "/a/{x}{y}", "/a/}"] {
            assert!(axum_panics(&[p]), "{p}");
        }
    }
}
