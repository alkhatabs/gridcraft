//! The chrome around the grid — ribbon, formula bar, worksheet tab strip — mirrors on a
//! right-to-left sheet. The grid has done this since #146; these are the parts that did not.
//! Everything here is asserted on painted geometry, because the ribbon and the tabs are painted
//! text rather than focusable widgets.

use egui::{Color32, Event, PointerButton, Pos2, Rect, Shape, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, theme::Tokens};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// The whole window, with three sheets named `Tab00…`, the active cell in `B2`, and the direction
/// the sheet was told to read in.
fn harness(width: f32, rtl: bool) -> Harness {
    harness_sheets(width, rtl, 3)
}

/// The same window with a chosen number of sheets, for the tests that need the tab strip to
/// overflow its viewport.
fn harness_sheets(width: f32, rtl: bool, sheets: usize) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"sheet": 0, "name": "Tab00"})).unwrap();
    for i in 1..sheets {
        s.execute("home.insertSheet", json!({"name": format!("Tab{i:02}")})).unwrap();
    }
    s.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
    s.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut fonts_ready = false;
    let mut h = Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(width, 800.0)).build_ui_state(
        move |ui, app: &mut SheetApp| {
            if !fonts_ready {
                ui.ctx().set_fonts(gridcraft_ui_egui::theme::font_definitions());
                fonts_ready = true;
                return;
            }
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(6);
    h
}

/// Where a painted label sits. Ribbon tab labels and tab names are `ui.painter().text`, so the
/// shapes are the only handle on them.
fn painted(h: &Harness, text: &str) -> Option<Rect> {
    h.output().shapes.iter().find_map(|s| match &s.shape {
        Shape::Text(t) if t.galley.job.text == text => Some(t.visual_bounding_rect()),
        _ => None,
    })
}

fn expect(h: &Harness, text: &str) -> Rect {
    painted(h, text).unwrap_or_else(|| panic!("{text} was not painted"))
}

/// The ribbon card: the rect filled with the ribbon token that is the card's own height. The
/// window and the grid behind it use the same token, so height is what identifies it.
fn ribbon_card(h: &Harness) -> Rect {
    let t = Tokens::get(&h.ctx);
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Rect(r) if r.fill == t.ribbon && (80.0..=96.0).contains(&r.rect.height()) => Some(r.rect),
            _ => None,
        })
        .next()
        .expect("the ribbon card is painted")
}

/// Painted filled rectangles of one colour in a short band, widest first. The Name Box and the
/// formula field are drawn by hand in that colour, so their geometry is the assertion.
fn boxes(h: &Harness, fill: Color32, band: f32) -> Vec<Rect> {
    let mut out: Vec<Rect> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Rect(r) if r.fill == fill && r.rect.height() < band => Some(r.rect),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| b.width().total_cmp(&a.width()));
    out
}

/// (field, Name Box) — the two hand-painted boxes of the formula bar, by width.
fn pair(h: &Harness, bg: Color32) -> (Rect, Rect) {
    let found = boxes(h, bg, 40.0);
    let field = *found.iter().find(|r| r.width() > 400.0).unwrap_or_else(|| panic!("no formula field in {found:?}"));
    let name = *found.iter().find(|r| (r.width() - 118.0).abs() < 1.0).unwrap_or_else(|| panic!("no Name Box in {found:?}"));
    (field, name)
}

/// The workbook's sheet order, as names.
fn order(h: &Harness) -> Vec<String> {
    h.state().session.active().map(|d| d.wb.sheets.iter().map(|s| s.name.clone()).collect()).unwrap_or_default()
}

/// Press in the middle of `from`, move to `to` in four steps, release.
fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    let step = (to - from) / 4.0;
    h.input_mut().events.push(Event::PointerMoved(from));
    h.run_steps(1);
    h.input_mut().events.push(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Default::default() });
    h.run_steps(1);
    for i in 1..=4 {
        h.input_mut().events.push(Event::PointerMoved(from + step * i as f32));
        h.run_steps(1);
    }
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Default::default() });
    h.run_steps(2);
}

#[test]
fn the_worksheet_tab_strip_reads_right_to_left() {
    let h = harness(1200.0, true);
    let (a, b, c) = (expect(&h, "Tab00"), expect(&h, "Tab01"), expect(&h, "Tab02"));
    assert!(a.min.x > b.min.x && b.min.x > c.min.x, "tabs should run right to left: {a:?} {b:?} {c:?}");
}

#[test]
fn the_worksheet_tab_strip_keeps_its_order_left_to_right() {
    let h = harness(1200.0, false);
    let (a, b, c) = (expect(&h, "Tab00"), expect(&h, "Tab01"), expect(&h, "Tab02"));
    assert!(a.min.x < b.min.x && b.min.x < c.min.x, "a left-to-right sheet must be untouched: {a:?} {b:?} {c:?}");
}

#[test]
fn the_ribbon_tab_strip_reads_from_the_right_edge() {
    let left = harness(1200.0, false);
    let right = harness(1200.0, true);
    let (home_l, insert_l) = (expect(&left, "Home"), expect(&left, "Insert"));
    let (home_r, insert_r) = (expect(&right, "Home"), expect(&right, "Insert"));
    assert!(home_l.min.x < insert_l.min.x, "left-to-right ribbon: {home_l:?} {insert_l:?}");
    assert!(home_r.min.x > insert_r.min.x, "mirrored ribbon tab strip: {home_r:?} {insert_r:?}");
}

#[test]
fn the_group_strip_keeps_its_order_because_its_scroll_area_cannot_mirror() {
    // Pinned, not preferred: `ScrollArea` has no direction. Measured on the Home tab at 1200 px,
    // the groups are wider than the card, and laying them out right to left put the last groups
    // off the left edge (painted shapes reaching x = -365) while a horizontal wheel moved the
    // content further away instead of into view. The groups stay left to right until egui has a
    // right-to-left scroll viewport.
    for width in [520.0, 1200.0] {
        let h = harness(width, true);
        let card = ribbon_card(&h);
        let paste = expect(&h, "Paste");
        assert!(paste.center().x < card.center().x, "the first group stays at the left: {paste:?} in {card:?}");
        assert!(paste.min.x >= card.min.x && paste.max.x <= card.max.x, "the first group is inside the card: {paste:?} in {card:?}");
    }
}

#[test]
fn the_ribbon_card_keeps_its_own_width_on_a_narrow_window() {
    // The card is as wide as the row, not as wide as the tab strip that overflows that row.
    // A mirrored strip overflows to the left, and deriving the card from the cursor the strip
    // left behind moved it off screen with it: -135 px at a 520 px window.
    for width in [520.0, 1200.0] {
        let left = harness(width, false);
        let right = harness(width, true);
        let (card_l, card_r) = (ribbon_card(&left), ribbon_card(&right));
        assert_eq!(card_l, card_r, "the card spans the same place whatever the sheet reads in");
        assert!(card_l.min.x >= 0.0 && card_l.max.x <= width, "the card is inside a {width} px window: {card_l:?}");
    }
}

#[test]
fn the_name_box_and_the_formula_field_swap_sides() {
    let bg = Tokens::get(&harness(1200.0, false).ctx).input_bg;
    let left = harness(1200.0, false);
    let right = harness(1200.0, true);
    let (field_l, name_l) = pair(&left, bg);
    let (field_r, name_r) = pair(&right, bg);
    assert!(name_l.min.x < field_l.min.x, "left-to-right bar: Name Box {name_l:?} then field {field_l:?}");
    assert!(name_r.min.x > field_r.min.x, "mirrored bar: field {field_r:?} then Name Box {name_r:?}");
    // Both are still on screen and neither lost its width to the mirroring.
    for (field, name) in [(field_l, name_l), (field_r, name_r)] {
        assert!((name.width() - 118.0).abs() < 1.0, "the Name Box keeps its width: {name:?}");
        assert!(field.width() > 400.0, "the formula field keeps the rest of the bar: {field:?}");
        assert!(field.min.x >= 0.0 && field.max.x <= 1200.0, "the field is inside the window: {field:?}");
    }
}

#[test]
fn dragging_a_tab_towards_the_end_moves_it_towards_the_end() {
    // One slot to the right means one slot later in the workbook on a left-to-right sheet and one
    // slot *earlier* on a mirrored one, because there the strip reads the other way. Measured on
    // the fifth tab of eight, a slot apart from its neighbour and inside the strip at this width.
    let mut left = harness_sheets(420.0, false, 8);
    let tab_l = expect(&left, "Tab05").center();
    drag(&mut left, tab_l, tab_l + vec2(72.0, 0.0));
    assert_eq!(
        order(&left),
        ["Tab00", "Tab01", "Tab02", "Tab03", "Tab04", "Tab06", "Tab05", "Tab07"],
        "left-to-right: a drag right moves the sheet later: {:?}",
        order(&left)
    );

    let mut right = harness_sheets(420.0, true, 8);
    let tab_r = expect(&right, "Tab05").center();
    drag(&mut right, tab_r, tab_r + vec2(72.0, 0.0));
    assert_eq!(
        order(&right),
        ["Tab00", "Tab01", "Tab02", "Tab03", "Tab05", "Tab04", "Tab06", "Tab07"],
        "mirrored: the same drag right moves the sheet earlier, towards the tab it reads after: {:?}",
        order(&right)
    );
}

#[test]
fn the_scrollbar_track_moves_to_the_grids_left_edge() {
    let left = harness(1200.0, false);
    let right = harness(1200.0, true);
    let (track_l, track_r) = (
        left.state().grid.hscroll.expect("the tab bar has a horizontal scrollbar"),
        right.state().grid.hscroll.expect("the tab bar has a horizontal scrollbar"),
    );
    assert!(track_l.max.x <= 1200.0 && track_r.min.x >= 0.0, "{track_l:?} {track_r:?}");
    let grid_right_edge = left.state().grid.rect.expect("grid").max.x;
    let grid_left_edge = right.state().grid.rect.expect("grid").min.x;
    assert!(
        (track_l.max.x - grid_right_edge).abs() < 30.0,
        "left-to-right: the track ends where the grid does: {track_l:?} against {grid_right_edge}"
    );
    assert!(
        (track_r.min.x - grid_left_edge).abs() < 30.0,
        "mirrored: the track starts where the grid does: {track_r:?} against {grid_left_edge}"
    );
}
