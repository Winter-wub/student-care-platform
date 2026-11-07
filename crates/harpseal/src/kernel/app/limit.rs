//! Limits and the refusal log of the exchange (see [`super::exchange`]).
//!
//! - Clients are grouped by IP: an IPv4 address as is, an IPv6 address by
//!   its /64 (one subscriber's prefix, so address rotation does not help).
//! - [`verify_allowed`], before every verification on a Channel whose
//!   verification is a network call (#86), per IP group and epoch minute:
//!   - no verification once the group has had [`VERIFY_PER_MINUTE`]
//!     **refused** tokens (key `app.verify:<group>`, counted after the
//!     call by [`count_refusal`]), so a class behind one campus NAT is
//!     never slowed down by its own valid launches;
//!   - and every attempt, accepted, refused or failed, is counted first,
//!     atomically (`UPSERT ... RETURNING`, key `app.attempt:<group>`),
//!     against `[mini_app] verify_per_minute` (default 300). That ceiling
//!     is what bounds a burst sent all at once (the refusal count is only
//!     known after the calls) and a replayed valid token, so nobody can
//!     make LINE throttle the Deployment.
//! - [`log_refusal`]: at most one `app.session_refused` Security Event per
//!   group per minute (check, then insert: it is only a log, so a race
//!   writing two is harmless).

use std::net::IpAddr;

use crate::kernel::rate_limit;
use crate::port_types::Statement;
use crate::{Result, Services};

/// Refused tokens per IP group per minute on a networked Channel.
pub(crate) const VERIFY_PER_MINUTE: i64 = 30;
/// Key prefix in `rate_limit` of refused tokens.
const VERIFY_KEY: &str = "app.verify:";
/// Key prefix in `rate_limit` of every attempt.
const ATTEMPT_KEY: &str = "app.attempt:";
/// Security Event kind of a refused token.
pub(crate) const REFUSED: &str = "app.session_refused";
const MINUTE_MS: i64 = 60_000;

/// The group an address counts in: IPv4 as is, IPv6 as its /64.
pub(crate) fn ip_group(ip: &str) -> String {
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) if v6.to_ipv4_mapped().is_none() => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
        Ok(IpAddr::V6(v6)) => v6
            .to_ipv4_mapped()
            .map_or_else(String::new, |v4| v4.to_string()),
        Ok(IpAddr::V4(v4)) => v4.to_string(),
        Err(_) => ip.to_string(),
    }
}

/// `false` once `group` has had the minute's refused tokens, or (counting
/// this attempt) more than `ceiling` attempts this minute.
pub(crate) async fn verify_allowed(services: &Services, group: &str, ceiling: u32) -> Result<bool> {
    let window = services.now_ms().div_euclid(MINUTE_MS);
    let refused = rate_limit::count(services, &format!("{VERIFY_KEY}{group}"), window).await?;
    if refused >= VERIFY_PER_MINUTE {
        return Ok(false);
    }
    let n = rate_limit::hit(services, &format!("{ATTEMPT_KEY}{group}"), window).await?;
    Ok(n <= i64::from(ceiling))
}

/// Counts one refused token of `group` now.
pub(crate) async fn count_refusal(services: &Services, group: &str) -> Result<()> {
    let window = services.now_ms().div_euclid(MINUTE_MS);
    rate_limit::hit(services, &format!("{VERIFY_KEY}{group}"), window)
        .await
        .map(|_| ())
}

/// Writes `app.session_refused` (Channel kind and IP group, never the
/// token) unless this group already has one in the current minute.
pub(crate) async fn log_refusal(
    services: &Services,
    channel: &str,
    group: Option<&str>,
) -> Result<()> {
    let now = services.now_ms();
    let start = now - now.rem_euclid(MINUTE_MS);
    let check = Statement::new(
        "SELECT 1 FROM security_event WHERE at >= ? AND kind = ? \
         AND json_extract(details, '$.ip') IS ? LIMIT 1",
    )
    .bind(start)
    .bind(REFUSED)
    .bind(group.map(str::to_string));
    if !services.db.query(check).await?.is_empty() {
        return Ok(());
    }
    let mut details = vec![("channel", channel.to_string())];
    if let Some(g) = group {
        details.push(("ip", g.to_string()));
    }
    crate::kernel::security::record(services, REFUSED, None, None, &details).await
}

/// Deletes counter windows older than an hour (at most 500 rows).
pub(crate) async fn prune(services: &Services, now: i64) -> Result<u64> {
    let before = now.div_euclid(MINUTE_MS) - 60;
    let refused = rate_limit::prune(services, VERIFY_KEY, before, 500).await?;
    Ok(refused + rate_limit::prune(services, ATTEMPT_KEY, before, 500).await?)
}

#[cfg(test)]
mod tests {
    use super::ip_group;

    #[test]
    fn ipv6_counts_per_slash_64() {
        assert_eq!(ip_group("203.0.113.9"), "203.0.113.9");
        assert_eq!(
            ip_group("2001:db8:aa:bb:1:2:3:4"),
            ip_group("2001:db8:aa:bb:ffff::1")
        );
        assert_eq!(ip_group("2001:db8:aa:bb::9"), "2001:db8:aa:bb::/64");
        assert_ne!(ip_group("2001:db8:aa:bc::1"), ip_group("2001:db8:aa:bb::1"));
        assert_eq!(ip_group("::ffff:198.51.100.4"), "198.51.100.4");
    }
}
