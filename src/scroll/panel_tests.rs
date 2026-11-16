use crate::scroll::panel::{self, Slot};
use tiny_skia::Rect;

fn region(width: f32, height: f32) -> Rect {
    Rect::from_xywh(0.0, 0.0, width, height).unwrap()
}

fn place(region: Rect, monitor: (f32, f32), preview: Option<(u32, u32)>) -> panel::PanelLayout {
    let slot = panel::choose_slot(region, monitor);
    panel::layout(region, monitor, preview, slot, false)
}

fn at(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::from_xywh(x, y, width, height).unwrap()
}

#[test]
fn panel_sits_right_of_the_region_when_there_is_room() {
    let layout = place(region(400.0, 300.0), (1920.0, 1080.0), None);
    assert!(layout.rect.left() >= 400.0);
    assert!(layout.rect.right() <= 1920.0);
}

#[test]
fn panel_flips_to_the_left_when_the_right_edge_is_used_up() {
    // flips left when right space is less than panel width
    let layout = place(region(1900.0, 300.0), (1920.0, 1080.0), None);
    assert!(layout.rect.right() <= 1900.0);
    assert!(layout.rect.left() >= 0.0);
}

#[test]
fn full_screen_region_keeps_the_panel_inside_the_monitor() {
    let layout = place(region(1920.0, 1080.0), (1920.0, 1080.0), None);
    assert!(layout.rect.left() >= 0.0);
    assert!(layout.rect.right() <= 1920.0);
    assert!(layout.rect.top() >= 0.0);
    assert!(layout.rect.bottom() <= 1080.0);
}

#[test]
fn preview_reserves_vertical_space_and_buttons_stay_inside() {
    let with_preview = place(region(400.0, 300.0), (1920.0, 1080.0), Some((132, 400)));
    let without = place(region(400.0, 300.0), (1920.0, 1080.0), None);
    assert!(with_preview.rect.height() > without.rect.height());
    for (_, rect) in &with_preview.buttons {
        assert!(rect.right() <= with_preview.rect.right());
        assert!(rect.bottom() <= with_preview.rect.bottom());
    }
}

#[test]
fn status_and_length_use_separate_rows() {
    let layout = place(region(400.0, 300.0), (1920.0, 1080.0), Some((132, 400)));
    assert!(
        layout.length.top() >= layout.status.bottom(),
        "length row {:?} overlaps status row {:?}",
        layout.length,
        layout.status
    );
    assert!(
        layout
            .buttons
            .iter()
            .all(|(_, r)| r.top() >= layout.length.bottom())
    );
    // both rows stay within panel bounds
    assert!(layout.status.right() <= layout.rect.right());
    assert!(layout.length.right() <= layout.rect.right());
}

#[test]
fn hit_test_finds_buttons_and_nothing_outside() {
    let layout = place(region(400.0, 300.0), (1920.0, 1080.0), None);
    let (button, rect) = layout.buttons.first().expect("panel has buttons");
    let center = (
        (rect.left() + rect.width() / 2.0) as f64,
        (rect.top() + rect.height() / 2.0) as f64,
    );
    assert_eq!(panel::hit_test(&layout, center), Some(*button));
    assert_eq!(panel::hit_test(&layout, (0.0, 0.0)), None);
}

#[test]
fn toggle_axis_icon_switches_between_vertical_and_horizontal() {
    use crate::scroll::session::ScrollSession;
    use crate::ui::icons;

    let mut session = ScrollSession::new(region(40.0, 160.0), 0).unwrap();
    assert_eq!(
        panel::button_icon(panel::PanelButton::ToggleAxis, &session),
        icons::SCROLL
    );

    assert!(session.toggle_axis());
    assert_eq!(
        panel::button_icon(panel::PanelButton::ToggleAxis, &session),
        icons::SCROLL_HORIZONTAL
    );
}

#[test]
fn toggle_axis_availability_before_and_after_growth() {
    use crate::scroll::session::ScrollSession;

    let session = ScrollSession::new(region(40.0, 160.0), 0).unwrap();
    assert!(panel::is_button_enabled(
        panel::PanelButton::ToggleAxis,
        &session
    ));
    assert!(panel::is_button_enabled(panel::PanelButton::Edit, &session));
    assert!(panel::is_button_enabled(
        panel::PanelButton::Finish,
        &session
    ));
    assert!(panel::is_button_enabled(
        panel::PanelButton::Cancel,
        &session
    ));
}

#[test]
fn preview_window_keeps_the_last_frame_in_view() {
    // the thumbnail is 1000 rows, 220 of them fit
    let window = |focus| panel::preview_window(1000.0, 220.0, focus);
    // scrolling down: the frame placed last ends at the bottom
    assert_eq!(window((900.0, 1000.0)), 780.0);
    // scrolling up: it starts at the top
    assert_eq!(window((0.0, 100.0)), 0.0);
    // back somewhere in the middle: the window ends where the frame does
    assert_eq!(window((500.0, 600.0)), 380.0);
    // a frame taller than the window shows from its top
    assert_eq!(window((300.0, 700.0)), 300.0);
    // a thumbnail that fits is shown whole
    assert_eq!(panel::preview_window(100.0, 220.0, (50.0, 100.0)), 0.0);
}

#[test]
fn slots_go_right_left_below_above_then_inside() {
    let monitor = (1920.0, 1080.0);
    assert_eq!(
        panel::choose_slot(region(400.0, 300.0), monitor),
        Slot::Right
    );
    assert_eq!(
        panel::choose_slot(at(1500.0, 0.0, 420.0, 300.0), monitor),
        Slot::Left
    );
    assert_eq!(
        panel::choose_slot(at(100.0, 0.0, 1820.0, 300.0), monitor),
        Slot::Below
    );
    assert_eq!(
        panel::choose_slot(at(0.0, 400.0, 1920.0, 680.0), monitor),
        Slot::Above
    );
    assert_eq!(
        panel::choose_slot(region(1920.0, 1080.0), monitor),
        Slot::Inside
    );
}

#[test]
fn slot_needs_the_room_for_the_tallest_panel() {
    let monitor = (1920.0, 1080.0);
    let tall = panel::layout(
        at(0.0, 0.0, 1920.0, 400.0),
        monitor,
        Some((132, 5000)),
        Slot::Below,
        false,
    );
    assert_eq!(
        panel::choose_slot(at(0.0, 0.0, 1920.0, 400.0), monitor),
        Slot::Below
    );
    assert!(tall.rect.bottom() <= 1080.0);
}

#[test]
fn below_and_above_align_to_the_right_edge_of_the_region() {
    let monitor = (1920.0, 1080.0);
    let area = at(100.0, 400.0, 1000.0, 300.0);
    let below = panel::layout(area, monitor, None, Slot::Below, false);
    assert!(below.rect.top() > area.bottom());
    assert_eq!(below.rect.right(), area.right());
    let above = panel::layout(area, monitor, Some((132, 100)), Slot::Above, false);
    assert!(above.rect.bottom() < area.top());
    assert_eq!(above.rect.right(), area.right());
}

#[test]
fn inside_sits_in_the_bottom_right_corner() {
    let area = region(1920.0, 1080.0);
    let layout = panel::layout(area, (1920.0, 1080.0), None, Slot::Inside, false);
    let gap = crate::theme::size::margin();
    assert_eq!(layout.rect.right(), area.right() - gap);
    assert_eq!(layout.rect.bottom(), area.bottom() - gap);
}

#[test]
fn compact_panel_has_no_preview() {
    let area = region(1920.0, 1080.0);
    let compact = panel::layout(area, (1920.0, 1080.0), Some((132, 400)), Slot::Inside, true);
    let plain = panel::layout(area, (1920.0, 1080.0), None, Slot::Inside, false);
    assert!(compact.preview.is_none());
    assert_eq!(compact.rect, plain.rect);
}

#[test]
fn every_button_fits_the_panel() {
    let layout = place(region(400.0, 300.0), (1920.0, 1080.0), None);
    assert_eq!(layout.buttons.len(), 5);
    for (_, rect) in &layout.buttons {
        assert!(rect.right() <= layout.rect.right());
    }
}
