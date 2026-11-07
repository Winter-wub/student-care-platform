# A Module's settings

How a Module declares a Setting (GLOSSARY): a typed value for the whole
Deployment, such as the bank account Students pay fees into, that Staff
change from the back office instead of a release, each only with the
Permission the Module names for it. Core change in #75. Back to
[Building a Deployment](deployment.md).

## Declaring one

```rust
r.permission(Permission::new("fees.bank_account.edit", "Change the bank account"))
    .setting(
        "fees.bank_account",          // key: "<module name>.<name>", a-z 0-9 _ .
        SettingKind::AccountNumber,   // see the table below
        "000-0-00000-0",              // default until the first save
        "fees.bank_account.edit",     // the only Permission that changes it
        "fees.bank_account_label",    // label (catalog key, not the setting key)
        "fees.bank_account_help",     // help text (catalog key)
    );
```

A Setting holds one value for the whole Deployment, not one per Locale
(those are Texts). `build()` refuses (`BuildError::InvalidSetting`):

- a key that is not `<module name>.<name>` (a-z, 0-9, `_`, `.`, at most
  64 bytes), or declared twice;
- a `kernel.` key from any Module but the Kernel (a Module named
  `kernel.backup` cannot reach the Kernel's `kernel.backup.notified`);
- a key that is also a catalog text key: give the label its own key;
- a default its kind refuses or would change (it must be stored as is);
- an undeclared Permission, and a label or help key without a text in
  the default Locale.

## Kinds

Each kind is checked on the server on every save:

| Kind | Accepted | Read as |
|---|---|---|
| `Text` | one line, 1 to 500 characters | `SettingValue::Text` |
| `MultilineText` | 1 to 5 000 characters, CRLF stored as LF | `SettingValue::Text` |
| `Number` | a whole number (`i64`), no separators or decimals | `SettingValue::Number` |
| `Date` | a calendar day, `YYYY-MM-DD` | `SettingValue::Date` |
| `Url` | `https://` and a host, up to 500 characters | `SettingValue::Url` |
| `AccountNumber` | ASCII digits, `-` and spaces, at least one digit, up to 64 characters | `SettingValue::Text` |

Values are trimmed. No kind accepts control characters (a multi-line
text keeps its line breaks), nor characters that change how text reads
without being seen: bidi embeddings, overrides and isolates (U+202A to
U+202E, U+2066 to U+2069), the direction marks (U+200E, U+200F, U+061C),
zero-width characters (U+200B to U+200D, U+2060, U+FEFF) and the line
and paragraph separators (U+2028, U+2029). Without that, a value such as
`<U+202E>0-54321-0-000` would read as one account number on every page
and in the audit log while Students copy another. Use `AccountNumber` for
anything Students type into a banking app.

## The page

`/admin/settings` lists every Module's settings, grouped by Module, and
appears in the navigation for Staff who can see at least one:

- A setting has a Save button only for Staff holding its own Permission.
- Staff holding `kernel.setting.edit` see every setting read-only, with
  the Permission that edits it; that Permission alone never changes one (a
  POST is a 403, and nothing is written).
- Staff who can see none get a 403.
- The form carries the CSRF token and a compare-and-set token of the
  value it showed: when someone else saved first, the save is refused
  with the conflict message and nothing is written.
- A refused value is shown again with its error (422).
- Every change writes an audit row `setting.update` (target
  `setting`/`<key>:`) with the value before and after, in the same batch
  as the value.
- A saved value its kind now refuses (a restored row, a kind or check
  changed since) is flagged on the page, so Staff save it again.

## Reading

`state.settings()` in a page handler, `cx.settings()` in a Job and
`deployment.settings()` elsewhere return `Settings`:

- `settings.get(key)` gives the `SettingValue`, `text(key)` any kind as
  text and `number(key)` an `i64`.
- Each read goes to the database, so a change shows on the next page view
  or Job run.
- `Error::Invalid` for an undeclared key, for `number` on another kind,
  and for a saved value that fails its kind's check. Such a value is never
  replaced by the default, which could show Students an outdated bank
  account: show "not set" instead (the example does).

## Storage and backups

Values live in the Kernel's `setting` table with an empty Locale (no
migration); no row means the default. The Texts page never edits that
Locale: `deployment.toml` refuses an empty `default_locale` and a
`[texts.""]` table. Backups and restores carry the settings with the rest
of the table.

## The example

The Fees Module (`deployments/example/deployment/src/fees.rs`) declares
`fees.bank_name` (`Text`) and `fees.bank_account` (`AccountNumber`), both
edited with `fees.bank_account.edit`, which only the `Finance` Role holds
(Administrators see them read-only). The Student's Mini App page
`/app/fees` (`fees/student.rs`) shows them beside the Student's own
outstanding balance, or "not set" when a saved value fails its check.
