//! Links on the Mini App home (#72): every Module's
//! [`crate::ModuleRegistrar::mini_app_link`], checked by `build()` and
//! sorted once, rendered by [`super::view::home`] in the Student's Locale.

use crate::error::BuildError;
use crate::manifest::Catalog;
use crate::module::AppLinkSpec;

/// A checked link: a catalog key and a plain path under `/app/`.
#[derive(Debug, Clone)]
pub(crate) struct AppLink {
    pub(crate) label_key: &'static str,
    pub(crate) path: &'static str,
    pub(crate) order: u16,
}

/// Checks every Module's links against its own public routes and the
/// merged catalog; returns them sorted by `order` (stable, so equal orders
/// keep registration order: the Kernel first, then Modules as added).
///
/// `public` is (module, path) of every public route.
pub(crate) fn check(
    links: Vec<(&'static str, AppLinkSpec)>,
    public: &[(&'static str, &'static str)],
    catalog: &Catalog,
) -> Result<Vec<AppLink>, BuildError> {
    let locale = catalog.default_locale();
    let mut out = Vec::with_capacity(links.len());
    for (module, l) in links {
        // A page of its own: `/app/` alone is the Kernel's home.
        let below = l.path.len() > "/app/".len() && l.path.starts_with("/app/");
        let plain = below && super::safe_next(l.path) == l.path;
        let own = public.iter().any(|(m, p)| *m == module && *p == l.path);
        if !plain || !own {
            return Err(BuildError::InvalidRoute {
                module,
                path: l.path.into(),
            });
        }
        if !catalog.has(&locale, l.label_key) {
            return Err(BuildError::UnknownText {
                source: format!("module `{module}`"),
                key: l.label_key.into(),
            });
        }
        out.push(AppLink {
            label_key: l.label_key,
            path: l.path,
            order: l.order,
        });
    }
    out.sort_by_key(|l| l.order);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{AppLinkSpec, BuildError, Catalog, check};
    use crate::Locale;

    fn catalog() -> Catalog {
        let mut c = Catalog::default();
        c.insert(Locale::th().as_str(), "m.a", "ก");
        c.insert(Locale::th().as_str(), "m.b", "ข");
        c
    }

    fn spec(label_key: &'static str, path: &'static str, order: u16) -> AppLinkSpec {
        AppLinkSpec {
            label_key,
            path,
            order,
        }
    }

    #[test]
    fn links_sort_by_order_then_registration() {
        let public = [("m", "/app/a"), ("m", "/app/b"), ("n", "/app/c")];
        let links = vec![
            ("m", spec("m.a", "/app/a", 20)),
            ("m", spec("m.b", "/app/b", 10)),
            ("n", spec("m.a", "/app/c", 20)),
        ];
        let got: Vec<_> = check(links, &public, &catalog())
            .unwrap()
            .iter()
            .map(|l| l.path)
            .collect();
        assert_eq!(got, ["/app/b", "/app/a", "/app/c"]);
    }

    #[test]
    fn a_link_outside_app_or_to_another_modules_page_is_refused() {
        let public = [
            ("m", "/app/a"),
            ("m", "/mine"),
            ("n", "/app/n"),
            ("m", "/app/x/{id}"),
        ];
        for (module, path) in [
            ("m", "/mine"),
            ("m", "/app"),
            ("m", "/app/"),
            ("m", "/app/x/{id}"),
            ("m", "/app/n"),
            ("m", "/app/unregistered"),
        ] {
            let r = check(vec![(module, spec("m.a", path, 1))], &public, &catalog());
            assert!(
                matches!(r, Err(BuildError::InvalidRoute { .. })),
                "{path}: {r:?}"
            );
        }
        let r = check(vec![("m", spec("m.zz", "/app/a", 1))], &public, &catalog());
        assert!(matches!(r, Err(BuildError::UnknownText { .. })), "{r:?}");
    }
}
