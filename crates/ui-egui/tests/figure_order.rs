//! The grid and the formula bar hand an Arabic-Indic figure to the shaper reversed, because shaping
//! a right-to-left run reverses everything in it (`bidi`). The order the pixels land in cannot be
//! measured here — the test fonts carry no `Arab` coverage, so the digits draw as boxes — and is
//! checked in the browser build instead, recorded in `docs/localization-parity.md`. What these tests
//! pin is where the reversal is applied, and the one surface it must skip: the cell editor, whose
//! caret indexes the string it holds.

use egui::{Shape, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const ASCENDING: &str = "١٢٣٤٥";
const REVERSED: &str = "٥٤٣٢١";

/// The window showing one workbook whose `A1` holds `text`, with that cell selected so the formula
/// bar carries the same content.
fn harness_with(text: &str) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[text]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut fonts_ready = false;
    let mut h = Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(900.0, 700.0)).build_ui_state(
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

/// Every string the window painted this frame, as the layout jobs received it.
fn painted(h: &Harness) -> Vec<String> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}

fn painted_as(h: &Harness, text: &str) -> usize {
    painted(h).iter().filter(|p| p.as_str() == text).count()
}

#[test]
fn a_figure_is_painted_in_the_order_the_shaper_will_turn_back() {
    let h = harness_with(ASCENDING);
    let shapes = painted(&h);
    assert!(shapes.iter().any(|p| p == REVERSED), "the figure was handed over reversed; painted {shapes:?}");
    assert_eq!(painted_as(&h, ASCENDING), 0, "nothing may paint the figure in the order it is stored");
}

#[test]
fn editing_the_cell_shows_the_text_as_written() {
    let mut h = harness_with(ASCENDING);
    h.state_mut().begin_edit(None, false);
    h.run_steps(3);
    assert!(painted_as(&h, ASCENDING) > 0, "the editor holds the text the user typed, in the order they typed it");
}

#[test]
fn a_latin_cell_is_painted_exactly_as_stored() {
    let h = harness_with("Total 12,340");
    assert!(painted_as(&h, "Total 12,340") > 0, "a figure the shaper already draws left-to-right is left alone");
}
