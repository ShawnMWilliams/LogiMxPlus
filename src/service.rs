//! Service: decides when and where the mouse switches. Depends on traits only.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::domain::{HidppError, HostInfo, Result};

pub trait SwitchableDevice {
    fn host_info(&mut self) -> Result<HostInfo>;
    fn change_host(&mut self, slot: u8) -> Result<()>;
}

pub trait Keyboard: SwitchableDevice {
    fn enable_easy_switch_reporting(&mut self) -> Result<()>;
}

const DEBOUNCE: Duration = Duration::from_millis(1500);

/// Makes the mouse follow the keyboard's Easy-Switch keys.
///
/// `channel_map` maps a zero-based keyboard host slot to the zero-based mouse slot of
/// the same computer (identity when absent). If the mouse can't be reached when a key
/// is pressed (asleep, or its link is still coming up), the switch stays pending for
/// `pending_ttl` and is applied when the mouse connects. The keyboard returning
/// cancels it. Time is passed in, so behaviour is deterministic under test.
pub struct SwitchService {
    channel_map: HashMap<u8, u8>,
    pending_ttl: Duration,
    keyboard: Option<Box<dyn Keyboard>>,
    mouse: Option<Box<dyn SwitchableDevice>>,
    keyboard_host: Option<u8>,
    mouse_host: Option<u8>,
    pending: Option<(u8, Instant)>,
    last_press: Option<(u8, Instant)>,
}

impl SwitchService {
    pub fn new(channel_map: HashMap<u8, u8>, pending_ttl: Duration) -> Self {
        Self {
            channel_map,
            pending_ttl,
            keyboard: None,
            mouse: None,
            keyboard_host: None,
            mouse_host: None,
            pending: None,
            last_press: None,
        }
    }

    pub fn mouse_slot_for(&self, keyboard_slot: u8) -> u8 {
        self.channel_map
            .get(&keyboard_slot)
            .copied()
            .unwrap_or(keyboard_slot)
    }

    pub fn pending_slot(&self, now: Instant) -> Option<u8> {
        self.pending
            .filter(|(_, since)| now.duration_since(*since) <= self.pending_ttl)
            .map(|(slot, _)| slot)
    }

    pub fn keyboard_connected(&mut self, mut keyboard: Box<dyn Keyboard>) {
        if self.pending.take().is_some() {
            log::info!("keyboard is back on this computer; cancelling pending mouse switch");
        }
        let setup = keyboard
            .enable_easy_switch_reporting()
            .and_then(|()| keyboard.host_info());
        match setup {
            Ok(info) => {
                self.keyboard_host = Some(info.current);
                log::info!(
                    "keyboard ready on host slot {}; Easy-Switch reporting enabled",
                    info.current + 1
                );
            }
            Err(e) => log::warn!("could not set up keyboard: {e}"),
        }
        self.keyboard = Some(keyboard);
    }

    pub fn keyboard_disconnected(&mut self) {
        self.keyboard = None;
    }

    pub fn mouse_connected(&mut self, mut mouse: Box<dyn SwitchableDevice>, now: Instant) {
        self.mouse_host = mouse.host_info().ok().map(|i| i.current);
        self.mouse = Some(mouse);
        let pending = self.pending_slot(now);
        self.pending = None; // applied below, or expired
        if let Some(slot) = pending {
            log::info!(
                "mouse connected with a pending switch; sending it to channel {}",
                slot + 1
            );
            self.switch_mouse(slot, now); // may re-queue if the mouse is still unreachable
        }
    }

    pub fn mouse_disconnected(&mut self) {
        self.mouse = None;
    }

    /// The keyboard reported (or announced) Easy-Switch to zero-based `keyboard_slot`.
    pub fn easy_switch_pressed(&mut self, keyboard_slot: u8, now: Instant) {
        if let Some((slot, at)) = self.last_press {
            if slot == keyboard_slot && now.duration_since(at) < DEBOUNCE {
                return;
            }
        }
        self.last_press = Some((keyboard_slot, now));
        if Some(keyboard_slot) == self.keyboard_host {
            log::info!("Easy-Switch {} pressed: already on that host", keyboard_slot + 1);
            return;
        }
        let target = self.mouse_slot_for(keyboard_slot);
        log::info!(
            "Easy-Switch {} pressed: moving mouse to its channel {}",
            keyboard_slot + 1,
            target + 1
        );
        self.switch_mouse(target, now);
    }

    /// Manual switch: mouse first (the keyboard leaving takes our event source with it).
    pub fn switch_all(&mut self, keyboard_slot: u8, now: Instant) -> Result<()> {
        self.switch_mouse(self.mouse_slot_for(keyboard_slot), now);
        match self.keyboard.as_mut() {
            Some(kb) => kb.change_host(keyboard_slot),
            None => Ok(()),
        }
    }

    fn switch_mouse(&mut self, slot: u8, now: Instant) {
        let Some(mouse) = self.mouse.as_mut() else {
            log::info!(
                "mouse not connected here; switch to channel {} is pending",
                slot + 1
            );
            self.pending = Some((slot, now));
            return;
        };
        if self.mouse_host == Some(slot) {
            log::info!("mouse already on channel {}", slot + 1);
            return;
        }
        match mouse.change_host(slot) {
            Ok(()) => {
                self.mouse_host = Some(slot);
                log::info!("mouse sent to channel {}", slot + 1);
            }
            Err(e @ HidppError::Unreachable { .. }) => {
                log::info!(
                    "mouse unreachable ({e}); switch to channel {} is pending",
                    slot + 1
                );
                self.pending = Some((slot, now));
            }
            Err(e) => log::warn!("mouse refused the switch: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct State {
        current: u8,
        reachable: bool,
        switched_to: Vec<u8>,
        reporting: bool,
        host_queries: usize,
        broken_setup: bool,
    }

    #[derive(Clone)]
    struct Fake(Rc<RefCell<State>>);

    impl Fake {
        fn new(current: u8) -> Self {
            Self(Rc::new(RefCell::new(State {
                current,
                reachable: true,
                ..State::default()
            })))
        }
        fn switched(&self) -> Vec<u8> {
            self.0.borrow().switched_to.clone()
        }
    }

    impl SwitchableDevice for Fake {
        fn host_info(&mut self) -> Result<HostInfo> {
            let mut s = self.0.borrow_mut();
            s.host_queries += 1;
            if !s.reachable {
                return Err(HidppError::unreachable(Some(4), "asleep"));
            }
            Ok(HostInfo {
                count: 3,
                current: s.current,
            })
        }
        fn change_host(&mut self, slot: u8) -> Result<()> {
            let mut s = self.0.borrow_mut();
            if !s.reachable {
                return Err(HidppError::unreachable(Some(4), "asleep"));
            }
            s.switched_to.push(slot);
            Ok(())
        }
    }

    impl Keyboard for Fake {
        fn enable_easy_switch_reporting(&mut self) -> Result<()> {
            let mut s = self.0.borrow_mut();
            if s.broken_setup {
                return Err(HidppError::protocol(None, "nope"));
            }
            s.reporting = true;
            Ok(())
        }
    }

    fn svc(map: &[(u8, u8)]) -> SwitchService {
        SwitchService::new(map.iter().copied().collect(), Duration::from_secs(60))
    }

    fn setup(map: &[(u8, u8)], kb_host: u8, mouse_host: u8) -> (SwitchService, Fake, Fake, Instant) {
        let (mut s, kb, mouse, t0) = (
            svc(map),
            Fake::new(kb_host),
            Fake::new(mouse_host),
            Instant::now(),
        );
        s.keyboard_connected(Box::new(kb.clone()));
        s.mouse_connected(Box::new(mouse.clone()), t0);
        (s, kb, mouse, t0)
    }

    #[test]
    fn press_moves_mouse_to_same_channel() {
        let (mut s, kb, mouse, t0) = setup(&[], 1, 1);
        assert!(kb.0.borrow().reporting);
        s.easy_switch_pressed(0, t0);
        assert_eq!(mouse.switched(), vec![0]);
        assert!(kb.switched().is_empty(), "the keyboard switches itself");
    }

    #[test]
    fn channel_map_is_applied() {
        let (mut s, _, mouse, t0) = setup(&[(0, 2)], 1, 1);
        s.easy_switch_pressed(0, t0);
        assert_eq!(mouse.switched(), vec![2]);
    }

    #[test]
    fn pressing_current_host_key_does_nothing() {
        let (mut s, _, mouse, t0) = setup(&[], 1, 1);
        s.easy_switch_pressed(1, t0);
        assert!(mouse.switched().is_empty());
    }

    #[test]
    fn mouse_already_on_target_is_not_switched() {
        let (mut s, _, mouse, t0) = setup(&[], 1, 2);
        s.easy_switch_pressed(2, t0);
        assert!(mouse.switched().is_empty());
    }

    #[test]
    fn switch_does_not_query_mouse_on_hot_path() {
        let (mut s, _, mouse, t0) = setup(&[], 1, 1);
        let before = mouse.0.borrow().host_queries;
        s.easy_switch_pressed(0, t0);
        assert_eq!(mouse.switched(), vec![0]);
        assert_eq!(mouse.0.borrow().host_queries, before);
    }

    #[test]
    fn repeat_press_is_debounced() {
        let (mut s, _, mouse, t0) = setup(&[], 1, 1);
        s.easy_switch_pressed(0, t0);
        s.easy_switch_pressed(0, t0 + Duration::from_millis(100));
        assert_eq!(mouse.switched(), vec![0]);
    }

    #[test]
    fn unreachable_mouse_gets_switch_when_it_connects() {
        let (mut s, t0) = (svc(&[]), Instant::now());
        s.keyboard_connected(Box::new(Fake::new(1)));
        let mouse = Fake::new(1);
        mouse.0.borrow_mut().reachable = false;
        s.mouse_connected(Box::new(mouse.clone()), t0);
        s.easy_switch_pressed(2, t0);
        assert_eq!(s.pending_slot(t0), Some(2));
        mouse.0.borrow_mut().reachable = true;
        s.mouse_disconnected();
        s.mouse_connected(Box::new(mouse.clone()), t0 + Duration::from_secs(5));
        assert_eq!(mouse.switched(), vec![2]);
        assert_eq!(s.pending_slot(t0 + Duration::from_secs(5)), None);
    }

    #[test]
    fn pending_switch_expires() {
        let (mut s, t0) = (svc(&[]), Instant::now());
        s.keyboard_connected(Box::new(Fake::new(1)));
        s.easy_switch_pressed(0, t0); // no mouse here
        let mouse = Fake::new(1);
        s.mouse_connected(Box::new(mouse.clone()), t0 + Duration::from_secs(61));
        assert!(mouse.switched().is_empty());
    }

    #[test]
    fn keyboard_returning_cancels_pending() {
        let (mut s, t0) = (svc(&[]), Instant::now());
        s.keyboard_connected(Box::new(Fake::new(1)));
        s.easy_switch_pressed(0, t0);
        assert_eq!(s.pending_slot(t0), Some(0));
        s.keyboard_disconnected();
        s.keyboard_connected(Box::new(Fake::new(1)));
        assert_eq!(s.pending_slot(t0), None);
    }

    #[test]
    fn switch_all_moves_mouse_then_keyboard() {
        let (mut s, kb, mouse, t0) = setup(&[], 1, 1);
        s.switch_all(0, t0).unwrap();
        assert_eq!(mouse.switched(), vec![0]);
        assert_eq!(kb.switched(), vec![0]);
    }

    #[test]
    fn keyboard_setup_failure_is_tolerated() {
        let mut s = svc(&[]);
        let kb = Fake::new(1);
        kb.0.borrow_mut().broken_setup = true;
        s.keyboard_connected(Box::new(kb));
        assert!(s.keyboard.is_some());
    }
}
