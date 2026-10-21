//! Digit order inside right-to-left text.
//!
//! egui 0.36 has no bidirectional algorithm. It shapes each run with harfrust and lets the shaper
//! guess that run's direction from the run's own script: `epaint-0.36.2/src/text/text_layout.rs:1431`
//! calls `buffer.guess_segment_properties()` and shapes right after it. The two notes upstream still
//! carries say the rest — `text/font.rs:829` "TODO(emilk): heed bidi characters" and
//! `text/text_layout.rs:1363`, that runs are split by font face and that script-aware splitting waits
//! on RTL/bidi support. A face that covers Arabic therefore drags Arabic-Indic digits into the same
//! right-to-left run as the letters, and shaping that run reverses all of it. The letters want that.
//! A number does not: `U+0660..U+0669` is `Script=Arabic` with `Bidi_Class=AN`, and AN digits keep
//! left-to-right order in a right-to-left paragraph, as they do everywhere else.
//!
//! Measured in the browser build on 2026-10-21: a cell holding `١٢٣٤٥` painted the digits in the
//! order an Arabic reader reads as ٥٤٣٢١, and the neighbouring cell holding `٥٤٣٢١` painted the other
//! order — each matching what the other string should look like, read against isolated `١` and `٥` and
//! against the same strings drawn by the browser's own text stack. The stored string was right in both
//! cases and the `.xlsx` reads correctly in Excel, so only this pane read the figure backwards.
//!
//! The fix is to hand the shaper the number already reversed, so its reversal cancels out. It is
//! applied where a value is painted (`grid.rs`, `formula_bar.rs`), never to text being edited: a
//! `TextEdit` maps a caret to a byte index in the string it holds, and reordered contents would move
//! the caret off the character it belongs to.
//!
//! What it deliberately leaves alone:
//! - ASCII digits beside Arabic text. They reach this pane from the number formats, which are en-US,
//!   and they measured correct — the mixed cell `نمو 34٪` reads right — because the Latin face serves
//!   them in their own left-to-right run.
//! - Any other mixing (Hebrew, a figure split by an ASCII digit, digits inside a chart label or a
//!   worksheet tab). Those need the real algorithm, in a run splitter or upstream.

use std::borrow::Cow;

/// Reverse each Arabic-Indic figure in `text`, so shaping reverses it back.
///
/// A figure is a run of Arabic-Indic or extended Arabic-Indic digits, joined across the separators
/// that sit between digits and followed by a percent sign: U+066A written after a figure is a
/// terminator that rule W3 of the algorithm makes part of the number, so it travels with the digits it
/// qualifies. A string without such a digit — every Latin cell — comes back borrowed.
pub(crate) fn order_digits(text: &str) -> Cow<'_, str> {
    if !text.chars().any(is_arabic_digit) {
        return Cow::Borrowed(text);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if !is_arabic_digit(chars[i]) {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < chars.len() {
            let c = chars[end];
            let digit_after = chars.get(end + 1).is_some_and(|n| is_arabic_digit(*n));
            if is_arabic_digit(c) || (is_separator(c) && digit_after) {
                end += 1;
            } else {
                break;
            }
        }
        if chars.get(end).is_some_and(|c| *c == '\u{066A}') {
            end += 1;
        }
        out.extend(chars[start..end].iter().rev());
        i = end;
    }
    Cow::Owned(out)
}

/// Arabic-Indic `٠..٩` and the extended Persian/Urdu form `۰..۹`.
fn is_arabic_digit(c: char) -> bool {
    matches!(c, '\u{0660}'..='\u{0669}' | '\u{06F0}'..='\u{06F9}')
}

/// Thousands and decimal separators, in the pair the en-US formats emit and the pair an Arabic locale
/// emits. Only ever part of a figure when a digit follows it.
fn is_separator(c: char) -> bool {
    matches!(c, ',' | '.' | '\u{066B}' | '\u{066C}')
}

#[cfg(test)]
mod tests {
    use super::order_digits;

    fn ordered(text: &str) -> String {
        order_digits(text).into_owned()
    }

    #[test]
    fn a_bare_figure_is_reversed() {
        assert_eq!(ordered("١٢٣٤٥"), "٥٤٣٢١");
        assert_eq!(ordered("۵۴۳۲۱"), "۱۲۳۴۵");
    }

    #[test]
    fn a_figure_keeps_its_place_in_the_sentence() {
        // The letters stay in logical order: the shaper reverses the run, they need no help.
        assert_eq!(ordered("نمو ٣٤"), "نمو ٤٣");
        assert_eq!(ordered("الربع الأول ٢٠٢٦"), "الربع الأول ٦٢٠٢");
    }

    #[test]
    fn a_percent_written_after_the_figure_travels_with_it() {
        assert_eq!(ordered("٣٤٪"), "٪٤٣");
        assert_eq!(ordered("نمو ٣٤٪ في الربع"), "نمو ٪٤٣ في الربع");
    }

    #[test]
    fn separators_between_digits_stay_inside_the_figure() {
        assert_eq!(ordered("١٢٬٣٤٠"), "٠٤٣٬٢١");
        assert_eq!(ordered("١٢,٣٤"), "٤٣,٢١");
        // A separator with no digit after it is not part of the figure, so it keeps its place and
        // the run's own reversal puts it at the reading end of a right-to-left cell.
        assert_eq!(ordered("١٢,"), "٢١,");
    }

    #[test]
    fn two_figures_in_one_cell_are_reversed_separately() {
        assert_eq!(ordered("١٢ و ٣٤"), "٢١ و ٤٣");
    }

    #[test]
    fn latin_and_the_empty_string_are_left_alone() {
        assert_eq!(ordered("Total 12,340"), "Total 12,340");
        assert_eq!(ordered("نمو 34٪"), "نمو 34٪");
        assert_eq!(ordered(""), "");
    }

    #[test]
    fn a_string_without_the_script_is_returned_borrowed() {
        assert!(matches!(order_digits("نمو 34"), std::borrow::Cow::Borrowed(_)));
        assert!(matches!(order_digits("١٢"), std::borrow::Cow::Owned(_)));
    }

    #[test]
    fn reordering_keeps_the_text_the_same_length_and_letters() {
        for text in ["١٢٣٤٥", "نمو ٣٤٪", "الربع ٢٠٢٦ · سلطنة", "١٢,٣٤ نمو"] {
            let out = ordered(text);
            assert_eq!(out.chars().count(), text.chars().count(), "{text}");
            let mut before: Vec<char> = text.chars().collect();
            let mut after: Vec<char> = out.chars().collect();
            before.sort_unstable();
            after.sort_unstable();
            assert_eq!(before, after, "{text}");
        }
    }
}
