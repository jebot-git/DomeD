//! Shift-drag tiling in the target monitor's work area (outer frame coordinates).
use crate::Rect;
pub fn tile(work: Rect, pointer: [f32; 2], threshold: f32) -> Option<Rect> {
    let left = pointer[0] <= work.x + threshold;
    let right = pointer[0] >= work.x + work.w - threshold;
    let top = pointer[1] <= work.y + threshold;
    let bottom = pointer[1] >= work.y + work.h - threshold;
    if !(left || right || top || bottom) {
        return None;
    }
    let (x, w) = if left {
        (work.x, work.w / 2.)
    } else if right {
        (work.x + work.w / 2., work.w / 2.)
    } else {
        (work.x, work.w)
    };
    let (y, h) = if top {
        (work.y, work.h / 2.)
    } else if bottom {
        (work.y + work.h / 2., work.h / 2.)
    } else {
        (work.y, work.h)
    };
    // Keep the client above its minimum after reserving the compositor frame.
    (w >= 168. && h >= 134.).then_some(Rect { x, y, w, h })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn halves_and_quarters_respect_offset_monitors_and_reserved_bars() {
        let work = Rect {
            x: -1920.,
            y: -164.,
            w: 1920.,
            h: 984.,
        };
        assert_eq!(
            tile(work, [-1910., 300.], 24.),
            Some(Rect {
                x: -1920.,
                y: -164.,
                w: 960.,
                h: 984.
            })
        );
        assert_eq!(
            tile(work, [-5., 815.], 24.),
            Some(Rect {
                x: -960.,
                y: 328.,
                w: 960.,
                h: 492.
            })
        );
        assert_eq!(tile(work, [-1000., 0.], 24.), None);
        assert_eq!(
            tile(work, [-1000., -190.], 24.),
            Some(Rect {
                x: -1920.,
                y: -164.,
                w: 1920.,
                h: 492.
            })
        );
    }
}
