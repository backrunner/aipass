//! Presentation timing shared by the native toast and its deterministic tests.
use std::time::Duration;

pub const TOAST_LIFETIME: Duration = Duration::from_secs(6);

#[derive(Default, Debug)]
pub struct ToastTimer {
    pub visible: bool,
    hovered: bool,
    focused: bool,
    deadline: Option<Duration>,
}

impl ToastTimer {
    pub fn show(&mut self, now: Duration) {
        self.visible = true;
        self.schedule(now);
    }
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.deadline = None;
    }
    pub fn hover(&mut self, hovered: bool, now: Duration) {
        self.hovered = hovered;
        self.schedule(now);
    }
    pub fn focus(&mut self, focused: bool, now: Duration) {
        self.focused = focused;
        self.schedule(now);
    }
    fn schedule(&mut self, now: Duration) {
        self.deadline = (self.visible && !self.hovered && !self.focused)
            .then_some(now.saturating_add(TOAST_LIFETIME));
    }
    pub fn remaining(&self, now: Duration) -> Option<Duration> {
        self.deadline.map(|deadline| deadline.saturating_sub(now))
    }
    pub fn tick(&mut self, now: Duration) -> bool {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.dismiss();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }
    #[test]
    fn toast_lasts_six_seconds_and_replacement_restarts_its_deadline() {
        let mut timer = ToastTimer::default();
        timer.show(ms(0));
        assert!(!timer.tick(ms(5999)));
        timer.show(ms(5999));
        assert!(!timer.tick(ms(6000)));
        assert!(!timer.tick(ms(11998)));
        assert!(timer.tick(ms(11999)));
        assert!(!timer.visible);
    }
    #[test]
    fn both_hover_and_focus_must_leave_before_a_full_new_lifetime() {
        let mut timer = ToastTimer::default();
        timer.show(ms(0));
        timer.hover(true, ms(5000));
        timer.focus(true, ms(6000));
        timer.hover(false, ms(7000));
        assert!(!timer.tick(ms(20000)));
        timer.focus(false, ms(20000));
        assert!(!timer.tick(ms(25999)));
        assert!(timer.tick(ms(26000)));
    }
    #[test]
    fn dismissal_cannot_be_resurrected_by_a_pending_hover_or_focus_event() {
        let mut timer = ToastTimer::default();
        timer.show(ms(0));
        timer.dismiss();
        timer.hover(false, ms(20));
        timer.focus(false, ms(30));
        assert!(timer.remaining(ms(40)).is_none());
        assert!(!timer.visible);
    }
}
