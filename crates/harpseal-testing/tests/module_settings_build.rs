//! `build()` refuses a wrongly declared Module setting (#75): its key,
//! its default, its Permission or its texts.

use harpseal::{BuildError, CatalogEntry, Module, ModuleRegistrar, Permission, SettingKind};
use harpseal_testing::Harness;

/// Declares settings on a registrar.
type Declare = fn(&mut ModuleRegistrar);

/// A Module declaring settings through its function.
struct Bad(Declare);

impl Module for Bad {
    fn name(&self) -> &'static str {
        "bad"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(Permission::new("bad.edit", "Edit"))
            .messages(&[CatalogEntry::new("bad.label", "th", "ป้าย")]);
        (self.0)(r);
    }
}

#[test]
fn build_refuses_a_wrong_declaration() {
    use SettingKind::*;
    let cases: [(Declare, &str); 7] = [
        (
            |r| {
                r.messages(&[CatalogEntry::new("bad.n", "th", "ข้อความ")]);
                r.setting("bad.n", Text, "x", "bad.edit", "bad.label", "bad.label");
            },
            "text key",
        ),
        (
            |r| {
                r.setting("other.key", Text, "x", "bad.edit", "bad.label", "bad.label");
            },
            "key",
        ),
        (
            |r| {
                r.setting("bad.Key", Text, "x", "bad.edit", "bad.label", "bad.label");
            },
            "a-z",
        ),
        (
            |r| {
                r.setting("bad.n", Number, "1.5", "bad.edit", "bad.label", "bad.label");
            },
            "default",
        ),
        (
            |r| {
                r.setting("bad.n", Text, "x", "bad.nope", "bad.label", "bad.label");
            },
            "permission",
        ),
        (
            |r| {
                r.setting("bad.n", Text, "x", "bad.edit", "bad.label", "bad.help");
            },
            "bad.help",
        ),
        (
            |r| {
                r.setting("bad.n", Text, "x", "bad.edit", "bad.label", "bad.label");
                r.setting("bad.n", Text, "y", "bad.edit", "bad.label", "bad.label");
            },
            "twice",
        ),
    ];
    for (declare, why) in cases {
        let h = Harness::new();
        let Err(err) = h.builder().module(Bad(declare)).build() else {
            panic!("refused: {why}");
        };
        assert!(
            matches!(&err, BuildError::InvalidSetting { module: "bad", .. })
                && err.to_string().contains(why),
            "{err}"
        );
    }
}

/// A Module whose name starts like a Kernel key.
struct KernelLike;

impl Module for KernelLike {
    fn name(&self) -> &'static str {
        "kernel.backup"
    }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.permission(Permission::new("kernel.backup.x", "Edit"))
            .messages(&[CatalogEntry::new("kernel.backup.label", "th", "ป้าย")])
            .setting(
                "kernel.backup.notified",
                SettingKind::Text,
                "x",
                "kernel.backup.x",
                "kernel.backup.label",
                "kernel.backup.label",
            );
    }
}

#[test]
fn only_the_kernel_declares_kernel_keys() {
    let h = Harness::new();
    let Err(err) = h.builder().module(KernelLike).build() else {
        panic!("a Module reached the Kernel's `kernel.backup.notified` row");
    };
    assert!(err.to_string().contains("only the Kernel"), "{err}");
}
