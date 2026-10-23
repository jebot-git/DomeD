use dome::taskbar::PopupLeaveDelay;
use std::time::Duration;
fn ms(value: u64) -> Duration { Duration::from_millis(value) }
#[test]
fn xr_popup_leave_waits_without_restarting_on_outside_motion() {
    let mut timer = PopupLeaveDelay::default();
    assert!(!timer.update(Some((0,false)),true,ms(0)));
    assert!(!timer.update(Some((0,false)),false,ms(100)));
    assert!(!timer.update(Some((0,false)),false,ms(499)));
    assert!(timer.update(Some((0,false)),false,ms(500)));
}
#[test]
fn return_new_popup_and_closed_popup_cancel_old_deadlines() {
    let mut timer = PopupLeaveDelay::default();
    timer.update(Some((0,false)),false,ms(0));
    assert!(!timer.update(Some((0,false)),true,ms(350)));
    assert!(!timer.update(Some((0,false)),false,ms(500)));
    assert!(!timer.update(Some((1,false)),false,ms(800)));
    assert!(!timer.update(Some((1,true)),false,ms(1100)));
    assert!(!timer.update(None,false,ms(1600)));
    assert!(!timer.update(Some((1,true)),false,ms(1700)));
    assert!(timer.update(Some((1,true)),false,ms(2100)));
}
