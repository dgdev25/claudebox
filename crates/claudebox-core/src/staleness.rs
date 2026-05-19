use chrono::{DateTime, Utc};

/// The outcome of a kernel staleness check.
pub struct StalenessResult {
    /// `true` when the kernel is at or beyond the staleness threshold.
    pub is_stale: bool,
    /// Number of whole days since the kernel was built.
    /// Set to `i64::MAX` when `kernel_built_at` cannot be parsed.
    pub days_old: i64,
}

/// Returns a [`StalenessResult`] indicating whether the kernel recorded in
/// `kernel_built_at` (RFC 3339 string) has exceeded `threshold_days` since
/// it was built.
///
/// # Conservative failure policy
///
/// If `kernel_built_at` cannot be parsed as an RFC 3339 timestamp, the
/// function returns `StalenessResult { is_stale: true, days_old: i64::MAX }`
/// so that the caller treats an unknown age as stale and triggers a rebuild.
pub fn check_kernel_staleness(kernel_built_at: &str, threshold_days: u32) -> StalenessResult {
    let built_at: DateTime<Utc> = match DateTime::parse_from_rfc3339(kernel_built_at) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => {
            // Conservative: unknown age → treat as stale.
            return StalenessResult {
                is_stale: true,
                days_old: i64::MAX,
            };
        }
    };

    let now = Utc::now();
    let age = now.signed_duration_since(built_at);
    let days_old = age.num_days();
    let is_stale = days_old >= threshold_days as i64;

    StalenessResult { is_stale, days_old }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staleness_warning_when_over_threshold() {
        let built_at = chrono::Utc::now() - chrono::Duration::days(91);
        let built_str = built_at.to_rfc3339();
        let result = check_kernel_staleness(&built_str, 90);
        assert!(result.is_stale);
        assert!(result.days_old >= 91);
    }

    #[test]
    fn test_staleness_ok_when_under_threshold() {
        let built_at = chrono::Utc::now() - chrono::Duration::days(5);
        let built_str = built_at.to_rfc3339();
        let result = check_kernel_staleness(&built_str, 90);
        assert!(!result.is_stale);
    }

    #[test]
    fn test_staleness_invalid_date_is_conservative() {
        let result = check_kernel_staleness("not-a-date", 90);
        assert!(result.is_stale);
        assert_eq!(result.days_old, i64::MAX);
    }

    #[test]
    fn test_staleness_exactly_at_threshold_is_stale() {
        // Exactly at the threshold counts as stale (>=).
        let built_at = chrono::Utc::now() - chrono::Duration::days(90);
        let built_str = built_at.to_rfc3339();
        let result = check_kernel_staleness(&built_str, 90);
        assert!(result.is_stale);
    }
}
