//! Pure scheduling policy: prioritize a pending input frame without starving playback.
use std::time::Duration;
#[derive(Debug, PartialEq)]
pub(super) enum BackgroundBudget {
    Yield,
    Limited,
    Normal,
}
pub(super) fn budget(
    input_age: Option<Duration>,
    pending: bool,
    since_service: Duration,
) -> BackgroundBudget {
    let Some(age) = input_age else {
        return BackgroundBudget::Normal;
    };
    if age < Duration::from_millis(32) && pending && since_service < Duration::from_millis(50) {
        BackgroundBudget::Yield
    } else if age < Duration::from_millis(80) {
        BackgroundBudget::Limited
    } else {
        BackgroundBudget::Normal
    }
}
/// Keep the established service interval during continuous input. Pending age is
/// the oldest request for the current serial, so new events cannot renew a yield
/// indefinitely. Pausing a held gesture restores the normal budget after 80ms.
pub(super) fn continuous_budget(
    input_age: Option<Duration>,
    pending_age: Option<Duration>,
    since_service: Duration,
) -> BackgroundBudget {
    let pending = pending_age.is_some_and(|age| age < Duration::from_millis(32));
    budget(input_age, pending, since_service)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_yields_until_present_but_background_has_a_service_deadline() {
        let ms = Duration::from_millis;
        assert_eq!(budget(None, true, ms(0)), BackgroundBudget::Normal);
        assert_eq!(budget(Some(ms(1)), true, ms(49)), BackgroundBudget::Yield);
        assert_eq!(budget(Some(ms(1)), true, ms(50)), BackgroundBudget::Limited);
        assert_eq!(budget(Some(ms(1)), false, ms(0)), BackgroundBudget::Limited);
        assert_eq!(budget(Some(ms(32)), true, ms(0)), BackgroundBudget::Limited);
        assert_eq!(budget(Some(ms(80)), true, ms(0)), BackgroundBudget::Normal);
    }
    #[test]
    fn continuous_requests_do_not_renew_pending_deadline_or_idle_limit() {
        let ms = Duration::from_millis;
        assert_eq!(
            continuous_budget(Some(ms(0)), Some(ms(1)), ms(49)),
            BackgroundBudget::Yield
        );
        assert_eq!(
            continuous_budget(Some(ms(0)), Some(ms(1)), ms(50)),
            BackgroundBudget::Limited
        );
        assert_eq!(
            continuous_budget(Some(ms(0)), Some(ms(32)), ms(0)),
            BackgroundBudget::Limited
        );
        assert_eq!(
            continuous_budget(Some(ms(0)), None, ms(0)),
            BackgroundBudget::Limited
        );
        assert_eq!(
            continuous_budget(Some(ms(0)), Some(ms(33)), ms(0)),
            BackgroundBudget::Limited
        );
        assert_eq!(
            continuous_budget(Some(ms(80)), Some(ms(80)), ms(0)),
            BackgroundBudget::Normal
        );
        assert_eq!(
            continuous_budget(None, None, ms(0)),
            BackgroundBudget::Normal
        );
    }
}
