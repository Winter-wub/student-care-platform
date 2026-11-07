//! The `harpseal` command line, as a library each Deployment's binary
//! calls with its own built [`Deployment`] (the example's is
//! `deployments/example/native/src/bin/harpseal.rs`):
//!
//! ```text
//! harpseal migrate
//! harpseal staff create --admin --email admin@sealbay.example [--name "Ada"]
//!                       [--password PW | --password-stdin] [--force]
//! harpseal staff create --email officer@sealbay.example --role Officer [--role ..]
//! harpseal backup ./backups/2026-11-07
//! harpseal restore ./backups/2026-11-07 [--force] [--drop-sessions]
//! ```
//!
//! `staff create` migrates first. Without `--password` or
//! `--password-stdin` it generates a temporary password, prints it once and
//! the Staff member must change it at first sign-in. `--admin` gives the
//! `Administrator` Role, creating it with every declared Permission when
//! the database has none. When the Role already exists (usually seeded
//! from `[roles]` in `deployment.toml`), the TOML wins: the Role is left as
//! it is and a warning lists the declared Permissions it lacks. `--admin`
//! is refused once any Staff exists unless `--force` is given. Passwords on the command line end up in shell
//! history: prefer `--password-stdin` or the generated one.
//!
//! `backup DIR` writes every table and every FileStore object a row
//! points at to a new (or empty) directory, through the Deployment's ports
//! ([`harpseal::backup`] has the format). Stop the server first: the dump
//! is not one transaction. The directory holds password hashes and session
//! ids, so keep it as safe as the database. `restore DIR` loads it into an
//! empty database (fresh, not even `migrate`d: `migrate` seeds `[roles]`),
//! applying the backup's migrations first and the newer ones after the
//! rows; `--force` deletes the rows of every table in the backup first and
//! keeps the schema and any table not in the backup; `--drop-sessions`
//! ends every restored Staff and App Session. Stop the server for both. Both print one line per table. D1 Deployments: this binary runs
//! natively with no D1 adapter, so use `wrangler d1 export` for now.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod backup;

use std::path::PathBuf;

use harpseal::{ADMINISTRATOR_ROLE, Actor, Deployment, NewStaff, RequestCtx};

pub use backup::{DirBackup, backup, restore};

/// How to run it.
pub const USAGE: &str = "usage:
  harpseal migrate
  harpseal staff create --email EMAIL [--admin] [--role ROLE]... [--name NAME]
                        [--password PW | --password-stdin] [--force]
  harpseal backup DIR
  harpseal restore DIR [--force] [--drop-sessions]";

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// `migrate`.
    Migrate,
    /// `staff create ..`.
    CreateStaff(StaffArgs),
    /// `backup DIR`.
    Backup(PathBuf),
    /// `restore DIR [--force] [--drop-sessions]`.
    Restore {
        /// The backup directory.
        dir: PathBuf,
        /// Delete the rows of every table in the backup first.
        force: bool,
        /// End every restored Staff and App Session.
        drop_sessions: bool,
    },
    /// `help`, `-h`, `--help`.
    Help,
}

/// Where the new Staff member's password comes from.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PasswordSource {
    /// `--password PW`.
    Given(String),
    /// `--password-stdin`: one line from standard input.
    Stdin,
    /// Neither: a temporary password is generated and printed once.
    Generate,
}

impl core::fmt::Debug for PasswordSource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            PasswordSource::Given(_) => "Given([redacted])",
            PasswordSource::Stdin => "Stdin",
            PasswordSource::Generate => "Generate",
        })
    }
}

/// Options of `staff create`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StaffArgs {
    /// `--email`.
    pub email: String,
    /// `--name`; the email's local part otherwise.
    pub name: Option<String>,
    /// `--admin`: the Administrator Role.
    pub admin: bool,
    /// `--role`, repeatable.
    pub roles: Vec<String>,
    /// `--password` / `--password-stdin` / generated.
    pub password: PasswordSource,
    /// `--force`: allow `--admin` when Staff already exist.
    pub force: bool,
}

impl StaffArgs {
    /// `staff create --email EMAIL` with nothing else.
    pub fn new(email: &str) -> Self {
        Self {
            email: email.into(),
            name: None,
            admin: false,
            roles: Vec::new(),
            password: PasswordSource::Generate,
            force: false,
        }
    }
}

/// Why a command failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum CliError {
    /// The command line is wrong; print [`USAGE`].
    Usage(String),
    /// The command was understood and refused.
    Refused(String),
    /// The Core failed.
    Core(harpseal::Error),
}

impl core::fmt::Display for CliError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CliError::Usage(m) => write!(f, "{m}\n{USAGE}"),
            CliError::Refused(m) => f.write_str(m),
            CliError::Core(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<harpseal::Error> for CliError {
    fn from(e: harpseal::Error) -> Self {
        CliError::Core(e)
    }
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, CliError> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["help" | "-h" | "--help", ..] => Ok(Command::Help),
        ["migrate"] => Ok(Command::Migrate),
        ["staff", "create", rest @ ..] => parse_staff(rest).map(Command::CreateStaff),
        ["backup", dir] if !dir.starts_with('-') => Ok(Command::Backup(dir.into())),
        ["restore", rest @ ..] => parse_restore(rest),
        _ => Err(CliError::Usage(format!(
            "unknown command `{}`",
            args.join(" ")
        ))),
    }
}

fn parse_restore(rest: &[&str]) -> Result<Command, CliError> {
    let (mut dir, mut force, mut drop_sessions) = (None, false, false);
    for w in rest {
        match *w {
            "--force" => force = true,
            "--drop-sessions" => drop_sessions = true,
            flag if flag.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{flag}`")));
            }
            d if dir.is_none() => dir = Some(PathBuf::from(d)),
            _ => return Err(CliError::Usage("restore takes one directory".into())),
        }
    }
    let dir = dir.ok_or_else(|| CliError::Usage("restore needs a directory".into()))?;
    Ok(Command::Restore {
        dir,
        force,
        drop_sessions,
    })
}

fn parse_staff(rest: &[&str]) -> Result<StaffArgs, CliError> {
    let mut a = StaffArgs::new("");
    let mut it = rest.iter();
    let value = |flag: &str, v: Option<&&str>| {
        v.map(|v| v.to_string())
            .ok_or_else(|| CliError::Usage(format!("{flag} needs a value")))
    };
    while let Some(flag) = it.next() {
        match *flag {
            "--email" => a.email = value(flag, it.next())?,
            "--name" => a.name = Some(value(flag, it.next())?),
            "--role" => a.roles.push(value(flag, it.next())?),
            "--password" => a.password = PasswordSource::Given(value(flag, it.next())?),
            "--password-stdin" => a.password = PasswordSource::Stdin,
            "--admin" => a.admin = true,
            "--force" => a.force = true,
            other => return Err(CliError::Usage(format!("unknown option `{other}`"))),
        }
    }
    if a.email.is_empty() {
        return Err(CliError::Usage("--email is required".into()));
    }
    Ok(a)
}

/// `migrate`: applies pending migrations and the `[roles]` seed; returns a
/// one-line report.
pub async fn migrate(d: &Deployment) -> Result<String, CliError> {
    let r = d.migrate().await?;
    Ok(format!(
        "migrations: {} applied, {} already done",
        r.applied.len(),
        r.skipped
    ))
}

/// What `staff create` did.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Created {
    /// `staff.id`.
    pub staff_id: String,
    /// The generated temporary password, to print once.
    pub temporary_password: Option<String>,
    /// Whether the Administrator Role had to be created.
    pub created_admin_role: bool,
    /// With `--admin` on an existing Administrator Role: declared
    /// Permissions it does not grant (the TOML wins; printed as a warning).
    pub missing_admin_permissions: Vec<String>,
}

impl core::fmt::Debug for Created {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Created")
            .field("staff_id", &self.staff_id)
            .field("created_admin_role", &self.created_admin_role)
            .field("missing_admin_permissions", &self.missing_admin_permissions)
            .finish_non_exhaustive()
    }
}

/// `staff create`. `password` is the resolved password (`None`
/// generates a temporary one); `a.password` only says where it came from.
pub async fn create_staff(
    d: &Deployment,
    a: &StaffArgs,
    password: Option<String>,
) -> Result<Created, CliError> {
    d.migrate().await?;
    let services = d.services();
    let ctx = RequestCtx::default();
    if a.admin && !a.force && harpseal::staff_count(services).await? > 0 {
        return Err(CliError::Refused(
            "Staff already exist; add --force to create another Administrator".into(),
        ));
    }
    let mut created_admin_role = false;
    let mut missing_admin_permissions = Vec::new();
    let mut roles = a.roles.clone();
    if a.admin {
        let all: Vec<&str> = d.permissions().iter().map(|p| p.id).collect();
        created_admin_role =
            harpseal::ensure_role(services, &Actor::System, ADMINISTRATOR_ROLE, &all, &ctx).await?;
        if !created_admin_role {
            let held = harpseal::role_permissions(services, ADMINISTRATOR_ROLE).await?;
            missing_admin_permissions = all
                .iter()
                .filter(|p| !held.iter().any(|h| h == *p))
                .map(|p| p.to_string())
                .collect();
        }
        roles.push(ADMINISTRATOR_ROLE.into());
    }
    let (password, temporary) = match password {
        Some(p) => (p, false),
        None => (temporary_password(d)?, true),
    };
    let name = a
        .name
        .clone()
        .unwrap_or_else(|| a.email.split('@').next().unwrap_or_default().to_string());
    let mut s = NewStaff::new(
        &a.email,
        &name,
        &password,
        d.config().default_locale.clone(),
    );
    s.roles = roles;
    s.must_change_password = temporary;
    let staff_id = harpseal::create_staff(services, &Actor::System, s, &ctx).await?;
    Ok(Created {
        staff_id,
        temporary_password: temporary.then_some(password),
        created_admin_role,
        missing_admin_permissions,
    })
}

/// 24 random hex digits in groups of six.
fn temporary_password(d: &Deployment) -> Result<String, CliError> {
    let hex = d.services().random_hex(12)?;
    let groups: Vec<&str> = (0..4).map(|i| &hex[i * 6..i * 6 + 6]).collect();
    Ok(groups.join("-"))
}

/// Parses `args`, runs the command and writes its output to `out`. Reads
/// `--password-stdin` from `stdin`.
pub async fn run(
    d: &Deployment,
    args: &[String],
    stdin: &mut dyn std::io::BufRead,
    out: &mut dyn std::io::Write,
) -> Result<(), CliError> {
    let io = |e: std::io::Error| CliError::Refused(format!("output: {e}"));
    match parse(args)? {
        Command::Help => writeln!(out, "{USAGE}").map_err(io),
        Command::Migrate => writeln!(out, "{}", migrate(d).await?).map_err(io),
        Command::Backup(dir) => backup(d, &dir, out).await,
        Command::Restore {
            dir,
            force,
            drop_sessions,
        } => restore(d, &dir, force, drop_sessions, out).await,
        Command::CreateStaff(a) => {
            let password = match &a.password {
                PasswordSource::Given(p) => Some(p.clone()),
                PasswordSource::Generate => None,
                PasswordSource::Stdin => {
                    let mut line = String::new();
                    stdin.read_line(&mut line).map_err(io)?;
                    Some(line.trim_end_matches(['\r', '\n']).to_string())
                }
            };
            let c = create_staff(d, &a, password).await?;
            writeln!(out, "created staff {} ({})", a.email.trim(), c.staff_id).map_err(io)?;
            if c.created_admin_role {
                writeln!(out, "created role {ADMINISTRATOR_ROLE}").map_err(io)?;
            }
            if !c.missing_admin_permissions.is_empty() {
                writeln!(
                    out,
                    "warning: role {ADMINISTRATOR_ROLE} comes from [roles] in deployment.toml and \
                     lacks: {} (add them there; the TOML wins)",
                    c.missing_admin_permissions.join(", ")
                )
                .map_err(io)?;
            }
            if let Some(p) = c.temporary_password {
                writeln!(
                    out,
                    "temporary password (shown once, change it at first sign-in): {p}"
                )
                .map_err(io)?;
            }
            Ok(())
        }
    }
}
