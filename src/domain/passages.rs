use crate::{CommonplaceError, Result};

pub const PASSAGE_TARGET_BYTES: usize = 1024;
const PASSAGE_OVERLAP_BYTES: usize = PASSAGE_TARGET_BYTES / 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassageRange {
    pub ordinal: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

impl PassageRange {
    pub fn text<'a>(&self, text: &'a str) -> Result<&'a str> {
        text.get(self.start_byte..self.end_byte).ok_or_else(|| {
            CommonplaceError::InvalidInput(
                "passage offsets must select an exact UTF-8 range".into(),
            )
        })
    }
}

pub fn prepare(text: &str, maximum_passages: usize) -> Result<Vec<PassageRange>> {
    let mut ranges = Vec::new();
    let mut pending_start = 0;
    let mut pending_end = 0;
    let mut paragraph_start = 0;
    let mut position = 0;
    let mut emit = |start, end| -> Result<()> {
        if start == end {
            return Ok(());
        }
        if ranges.len() >= maximum_passages {
            return Err(CommonplaceError::LimitExceeded(format!(
                "document exceeds the {maximum_passages}-passage limit"
            )));
        }
        ranges.push(PassageRange {
            ordinal: ranges.len(),
            start_byte: start,
            end_byte: end,
        });
        Ok(())
    };
    // split_inclusive retains every delimiter, including CR in CRLF.
    for line in text.split_inclusive('\n') {
        position += line.len();
        let blank = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or(line.strip_suffix('\n').unwrap_or(line))
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t'));
        if !blank && position != text.len() {
            continue;
        }
        let end = position;
        if end - paragraph_start > PASSAGE_TARGET_BYTES {
            emit(pending_start, pending_end)?;
            split_oversized(text, paragraph_start, end, &mut emit)?;
            pending_start = end;
            pending_end = end;
        } else {
            if end - pending_start > PASSAGE_TARGET_BYTES {
                emit(pending_start, pending_end)?;
                pending_start = paragraph_start;
            }
            pending_end = end;
        }
        paragraph_start = end;
    }
    emit(pending_start, pending_end)?;
    Ok(ranges)
}

fn split_oversized(
    text: &str,
    mut start: usize,
    end: usize,
    emit: &mut impl FnMut(usize, usize) -> Result<()>,
) -> Result<()> {
    // Reserve the target overlap while computing UTF-8-safe suffix capacity.
    // These bounds keep whitespace adjustments from leaving an oversized tail.
    let mut suffix_starts = vec![end];
    let mut position = end;
    while position > start {
        let mut previous = position.saturating_sub(PASSAGE_TARGET_BYTES).max(start);
        while !text.is_char_boundary(previous) {
            previous += 1;
        }
        suffix_starts.push(previous);
        if previous == start {
            break;
        }
        position = previous + PASSAGE_OVERLAP_BYTES;
    }
    let mut previous_end = start;
    for remaining in (2..suffix_starts.len()).rev() {
        let target_size =
            (end - start + PASSAGE_OVERLAP_BYTES * (remaining - 1)).div_ceil(remaining);
        let target = start + target_size;
        let lower = (suffix_starts[remaining - 1] + PASSAGE_OVERLAP_BYTES).max(previous_end + 1);
        let upper = (start + PASSAGE_TARGET_BYTES).min(end);
        let boundary = choose_boundary(text, start, lower, upper, target, target_size / 4)?;
        emit(start, boundary)?;
        let next_start = choose_boundary(
            text,
            start,
            suffix_starts[remaining - 1].max(start + 1),
            boundary - 1,
            boundary - PASSAGE_OVERLAP_BYTES,
            PASSAGE_OVERLAP_BYTES / 4,
        )?;
        previous_end = boundary;
        start = next_start;
    }
    emit(start, end)
}

fn choose_boundary(
    text: &str,
    start: usize,
    lower: usize,
    upper: usize,
    target: usize,
    whitespace_tolerance: usize,
) -> Result<usize> {
    let mut nearest = None;
    let mut whitespace = None;
    for (offset, character) in text[start..].char_indices() {
        let boundary = start + offset + character.len_utf8();
        if boundary > upper {
            break;
        }
        if boundary < lower {
            continue;
        }
        let key = (boundary.abs_diff(target), boundary);
        if nearest.is_none_or(|best| key < best) {
            nearest = Some(key);
        }
        if character.is_whitespace()
            && key.0 <= whitespace_tolerance
            && whitespace.is_none_or(|best| key < best)
        {
            whitespace = Some(key);
        }
    }
    whitespace
        .or(nearest)
        .map(|(_, boundary)| boundary)
        .ok_or_else(|| {
            CommonplaceError::InvalidInput("cannot prepare a UTF-8 passage boundary".into())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_exact_union(text: &str, ranges: &[PassageRange]) {
        let mut reconstructed = String::new();
        let mut previous_start = None;
        let mut covered_end = 0;
        for (ordinal, range) in ranges.iter().enumerate() {
            assert_eq!(range.ordinal, ordinal);
            assert!(range.start_byte <= covered_end);
            assert!(previous_start.is_none_or(|start| range.start_byte > start));
            assert!(range.end_byte > covered_end);
            assert!((1..=PASSAGE_TARGET_BYTES).contains(&(range.end_byte - range.start_byte)));
            assert_eq!(
                range.text(text).unwrap(),
                &text[range.start_byte..range.end_byte]
            );
            reconstructed.push_str(&text[covered_end..range.end_byte]);
            previous_start = Some(range.start_byte);
            covered_end = range.end_byte;
        }
        assert_eq!(covered_end, text.len());
        assert_eq!(reconstructed, text);
    }

    #[test]
    fn exact_coverage_for_unicode_and_all_line_endings() {
        let samples = [
            String::new(),
            "\u{feff}A\r\n\r\nNUL\0e\u{301}🦀\n \t\nlast".into(),
            format!("{}\r\n\r\n{}tail", "a".repeat(1020), "🦀".repeat(513)),
            "\n\n\n".into(),
            "\r".into(),
        ];
        for text in samples {
            let ranges = prepare(&text, 100).unwrap();
            assert_exact_union(&text, &ranges);
        }
    }

    #[test]
    fn golden_paragraph_packing_and_balanced_oversized_chunks() {
        let text = format!(
            "{}\n\n{}\r\n\r\n{}",
            "a".repeat(700),
            "b".repeat(500),
            "🦀".repeat(300)
        );
        let ranges = prepare(&text, 4).unwrap();
        assert_eq!(
            ranges
                .iter()
                .map(|r| (r.start_byte, r.end_byte))
                .collect::<Vec<_>>(),
            [(0, 702), (702, 1206), (1206, 1858), (1754, 2406)]
        );
        assert_eq!(prepare(&text, 3).unwrap_err().code(), "limit_exceeded");
        assert!(prepare("", 1).unwrap().is_empty());
        let text = format!("{}\n\n{}\n\n", "a".repeat(508), "b".repeat(512));
        assert_eq!(prepare(&text, 1).unwrap()[0].end_byte, 1024);
    }

    #[test]
    fn balances_without_greedy_tails_and_prefers_nearby_word_boundaries() {
        for (text, expected) in [
            ("a".repeat(1100), vec![601, 601]),
            ("a ".repeat(550), vec![600, 602]),
            (
                format!("{} {}", "a".repeat(549), "b".repeat(550)),
                vec![550, 652],
            ),
            (
                format!("{} {}", "a".repeat(545), "b".repeat(554)),
                vec![546, 656],
            ),
            (
                format!("{}\u{2003}{}", "a".repeat(547), "b".repeat(550)),
                vec![550, 652],
            ),
            (
                format!("{} {}", "a".repeat(99), "b".repeat(1000)),
                vec![601, 601],
            ),
            ("a ".repeat(1025), vec![752, 750, 752]),
            ("🦀".repeat(513), vec![752, 752, 756]),
            ("€".repeat(1024), vec![846, 843, 846, 843]),
        ] {
            let ranges = prepare(&text, expected.len()).unwrap();
            assert_eq!(
                ranges
                    .iter()
                    .map(|r| r.end_byte - r.start_byte)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(ranges, prepare(&text, expected.len()).unwrap());
            assert_exact_union(&text, &ranges);
            assert_eq!(
                prepare(&text, expected.len() - 1).unwrap_err().code(),
                "limit_exceeded"
            );
        }
        for text in ["s.", "list.", "lisher.", "🦀"] {
            assert_eq!(prepare(text, 1).unwrap()[0].text(text).unwrap(), text);
        }
    }

    #[test]
    fn adversarial_utf8_lengths_keep_exact_coverage_and_capacity() {
        for count in [255, 256, 257, 341, 342, 511, 512, 513, 683, 684, 1023, 1024] {
            for unit in ["a", "€", "🦀", "e\u{301}", "ab 🦀\t"] {
                let text = unit.repeat(count);
                let ranges = prepare(&text, 100).unwrap();
                assert_exact_union(&text, &ranges);
                assert_eq!(ranges, prepare(&text, 100).unwrap());
                if text.len() > PASSAGE_TARGET_BYTES {
                    assert!(
                        ranges
                            .iter()
                            .all(|range| range.end_byte - range.start_byte >= 400)
                    );
                }
            }
        }
    }

    #[test]
    fn exact_overlap_bytes_and_capacity_transitions_include_repeated_context() {
        for (length, expected) in [
            (1024, vec![(0, 1024)]),
            (1025, vec![(0, 564), (462, 1025)]),
            (1100, vec![(0, 601), (499, 1100)]),
            (1946, vec![(0, 1024), (922, 1946)]),
            (1947, vec![(0, 717), (615, 1332), (1230, 1947)]),
            (2048, vec![(0, 751), (649, 1400), (1298, 2048)]),
        ] {
            let text: String = (0..length)
                .map(|index| char::from(b'a' + (index % 26) as u8))
                .collect();
            let ranges = prepare(&text, 100).unwrap();
            assert_eq!(
                ranges
                    .iter()
                    .map(|range| (range.start_byte, range.end_byte))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_exact_union(&text, &ranges);
            for pair in ranges.windows(2) {
                let overlap = pair[0].end_byte - pair[1].start_byte;
                assert_eq!(overlap, PASSAGE_OVERLAP_BYTES);
                let left = pair[0].text(&text).unwrap();
                let right = pair[1].text(&text).unwrap();
                assert_eq!(&left[left.len() - overlap..], &right[..overlap]);
            }
        }
        let text = format!(
            "{} {} {}",
            "a".repeat(490),
            "b".repeat(109),
            "c".repeat(499)
        );
        let ranges = prepare(&text, 2).unwrap();
        assert_eq!(ranges[0].end_byte, 601);
        assert_eq!(ranges[1].start_byte, 491);
        assert_eq!(ranges[0].end_byte - ranges[1].start_byte, 110);
        assert_eq!(ranges[1].text(&text).unwrap().chars().next(), Some('b'));
        assert_exact_union(&text, &ranges);
    }
}
