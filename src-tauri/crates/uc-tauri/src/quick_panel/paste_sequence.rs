#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SimulatedKeyEvent {
    KeyDown(u16),
    KeyUp(u16),
}

pub(super) const VK_CONTROL_CODE: u16 = 0x11;
pub(super) const VK_MENU_CODE: u16 = 0x12;
pub(super) const VK_V_CODE: u16 = 0x56;

pub(super) fn ctrl_v_sequence(neutralize_alt: bool) -> Vec<SimulatedKeyEvent> {
    let mut events = Vec::with_capacity(if neutralize_alt { 6 } else { 4 });

    if neutralize_alt {
        events.push(SimulatedKeyEvent::KeyUp(VK_MENU_CODE));
    }

    events.push(SimulatedKeyEvent::KeyDown(VK_CONTROL_CODE));
    events.push(SimulatedKeyEvent::KeyDown(VK_V_CODE));
    events.push(SimulatedKeyEvent::KeyUp(VK_V_CODE));
    events.push(SimulatedKeyEvent::KeyUp(VK_CONTROL_CODE));

    if neutralize_alt {
        events.push(SimulatedKeyEvent::KeyDown(VK_MENU_CODE));
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_v_sequence_without_alt_sends_plain_ctrl_v() {
        assert_eq!(
            ctrl_v_sequence(false),
            vec![
                SimulatedKeyEvent::KeyDown(VK_CONTROL_CODE),
                SimulatedKeyEvent::KeyDown(VK_V_CODE),
                SimulatedKeyEvent::KeyUp(VK_V_CODE),
                SimulatedKeyEvent::KeyUp(VK_CONTROL_CODE),
            ]
        );
    }

    #[test]
    fn ctrl_v_sequence_with_alt_down_neutralizes_alt_around_ctrl_v() {
        assert_eq!(
            ctrl_v_sequence(true),
            vec![
                SimulatedKeyEvent::KeyUp(VK_MENU_CODE),
                SimulatedKeyEvent::KeyDown(VK_CONTROL_CODE),
                SimulatedKeyEvent::KeyDown(VK_V_CODE),
                SimulatedKeyEvent::KeyUp(VK_V_CODE),
                SimulatedKeyEvent::KeyUp(VK_CONTROL_CODE),
                SimulatedKeyEvent::KeyDown(VK_MENU_CODE),
            ]
        );
    }
}
