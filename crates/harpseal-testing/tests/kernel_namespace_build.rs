//! The `kernel.` namespace is the Kernel's (#103): `build()` refuses a
//! Module named `kernel` or `kernel.…`, and a Module's Permission, Action
//! id, Action prefix, text key or setting key starting with `kernel.`,
//! with an error naming the key.

use harpseal::{
    ActionContext, ActionId, BuildError, CatalogEntry, Message, Module, ModuleRegistrar,
    Permission, SettingKind,
};
use harpseal_testing::Harness;

/// Declares something on a registrar.
type Declare = fn(&mut ModuleRegistrar);

/// A Module of the given name, declaring through its function.
struct Named(&'static str, Declare);

impl Module for Named {
    fn name(&self) -> &'static str {
        self.0
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        (self.1)(r);
    }
}

async fn nothing(_cx: ActionContext) -> harpseal::Result<Vec<Message>> {
    Ok(Vec::new())
}

fn build(module: Named) -> Result<harpseal::Deployment, BuildError> {
    Harness::new().builder().module(module).build()
}

#[test]
fn a_module_named_like_the_kernel_is_refused() {
    for name in ["kernel", "kernel.x", "kernel.backup"] {
        let Err(err) = build(Named(name, |_| {})) else {
            panic!("`{name}` was accepted");
        };
        assert!(
            matches!(&err, BuildError::KernelNamespace { what: "name", key, .. } if key == name),
            "{err}"
        );
        assert!(err.to_string().contains(&format!("`{name}`")), "{err}");
    }
}

#[test]
fn a_permission_action_or_text_key_in_the_kernel_namespace_is_refused() {
    let cases: [(Declare, &str, &str); 6] = [
        (
            |r| {
                r.permission(Permission::new("kernel.mine.view", "View"));
            },
            "permission",
            "kernel.mine.view",
        ),
        (
            |r| {
                r.action(ActionId::new("kernel.mine.go").unwrap(), nothing);
            },
            "action",
            "kernel.mine.go",
        ),
        (
            |r| {
                r.action_prefix("kernel.mine.", nothing);
            },
            "action prefix",
            "kernel.mine.",
        ),
        (
            |r| {
                r.messages(&[CatalogEntry::new("kernel.mine.label", "th", "ป้าย")]);
            },
            "text key",
            "kernel.mine.label",
        ),
        (
            |r| {
                r.permission(Permission::new("mine.edit", "Edit"))
                    .messages(&[CatalogEntry::new("mine.label", "th", "ป้าย")])
                    .setting(
                        "kernel.consent.version",
                        SettingKind::Text,
                        "x",
                        "mine.edit",
                        "mine.label",
                        "mine.label",
                    );
            },
            "setting",
            "kernel.consent.version",
        ),
        (
            |r| {
                r.permission(Permission::new("mine.edit", "Edit"))
                    .messages(&[CatalogEntry::new("mine.label", "th", "ป้าย")])
                    .setting_without_default(
                        "kernel.consent.version",
                        SettingKind::Text,
                        "mine.edit",
                        "mine.label",
                        "mine.label",
                    );
            },
            "setting",
            "kernel.consent.version",
        ),
    ];
    for (declare, what, key) in cases {
        let Err(err) = build(Named("mine", declare)) else {
            panic!("{what} `{key}` was accepted");
        };
        let text = err.to_string();
        assert!(
            text.contains(key) && text.contains("mine"),
            "{what}: {text}"
        );
        match what {
            // Setting keys keep their own error.
            "setting" => assert!(
                matches!(&err, BuildError::InvalidSetting { module: "mine", .. }),
                "{err}"
            ),
            _ => assert!(
                matches!(&err, BuildError::KernelNamespace { what: w, key: k, .. }
                    if *w == what && k == key),
                "{err}"
            ),
        }
    }
}

#[test]
fn names_that_only_look_like_the_kernel_build() {
    // `kernel` as a word inside a name or key, not as the namespace.
    let ok = build(Named("kernels", |r| {
        r.permission(Permission::new("kernels.view", "View"))
            .permission(Permission::new("mine.kernel", "View"))
            .action(ActionId::new("kernelized.go").unwrap(), nothing)
            .messages(&[CatalogEntry::new("kernels.label", "th", "ป้าย")]);
    }));
    assert!(ok.is_ok(), "{:?}", ok.err());
}
