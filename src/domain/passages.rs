use crate::{CommonplaceError, Result};

pub const PASSAGE_TARGET_BYTES: usize = 1024;

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
    // Backward greedy cuts give the minimum feasible count and the earliest
    // endpoint that leaves enough capacity for each remaining UTF-8 suffix.
    let mut suffix_starts = vec![end];
    let mut position = end;
    while position > start {
        let mut previous = position.saturating_sub(PASSAGE_TARGET_BYTES).max(start);
        while !text.is_char_boundary(previous) {
            previous += 1;
        }
        suffix_starts.push(previous);
        position = previous;
    }
    for remaining in (2..suffix_starts.len()).rev() {
        let target_size = (end - start).div_ceil(remaining);
        let target = start + target_size;
        let lower = suffix_starts[remaining - 1];
        let upper = (start + PASSAGE_TARGET_BYTES).min(end);
        let mut nearest = None;
        let mut whitespace = None;
        for (offset, character) in text[start..end].char_indices() {
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
                && key.0 <= target_size / 4
                && whitespace.is_none_or(|best| key < best)
            {
                whitespace = Some(key);
            }
        }
        let boundary = whitespace
            .or(nearest)
            .ok_or_else(|| {
                CommonplaceError::InvalidInput("cannot prepare a UTF-8 passage boundary".into())
            })?
            .1;
        emit(start, boundary)?;
        start = boundary;
    }
    emit(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let mut reconstructed = String::new();
            for (ordinal, range) in ranges.iter().enumerate() {
                assert_eq!(range.ordinal, ordinal);
                assert_eq!(range.start_byte, reconstructed.len());
                assert!(range.end_byte > range.start_byte);
                assert!(range.end_byte - range.start_byte <= PASSAGE_TARGET_BYTES);
                reconstructed.push_str(range.text(&text).unwrap());
            }
            assert_eq!(reconstructed, text);
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
            [(0, 702), (702, 1206), (1206, 1806), (1806, 2406)]
        );
        assert_eq!(prepare(&text, 3).unwrap_err().code(), "limit_exceeded");
        assert!(prepare("", 1).unwrap().is_empty());
        let text = format!("{}\n\n{}\n\n", "a".repeat(508), "b".repeat(512));
        assert_eq!(prepare(&text, 1).unwrap()[0].end_byte, 1024);
    }

    #[test]
    fn balances_without_greedy_tails_and_prefers_nearby_word_boundaries() {
        for (text, expected) in [
            ("a".repeat(1100), vec![550, 550]),
            ("a ".repeat(550), vec![550, 550]),
            (
                format!("{} {}", "a".repeat(549), "b".repeat(550)),
                vec![550, 550],
            ),
            (
                format!("{} {}", "a".repeat(545), "b".repeat(554)),
                vec![546, 554],
            ),
            (
                format!("{}\u{2003}{}", "a".repeat(547), "b".repeat(550)),
                vec![550, 550],
            ),
            (
                format!("{} {}", "a".repeat(99), "b".repeat(1000)),
                vec![550, 550],
            ),
            ("a ".repeat(1025), vec![684, 682, 684]),
            ("🦀".repeat(513), vec![684, 684, 684]),
            // Three nominal byte chunks cannot fit these 3-byte characters.
            ("€".repeat(1024), vec![768, 768, 768, 768]),
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
            assert_eq!(
                ranges
                    .iter()
                    .map(|r| r.text(&text).unwrap())
                    .collect::<String>(),
                text
            );
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
                let mut minimum_chunks = 1;
                let mut packed_bytes = 0;
                for character in text.chars() {
                    if packed_bytes + character.len_utf8() > PASSAGE_TARGET_BYTES {
                        minimum_chunks += 1;
                        packed_bytes = 0;
                    }
                    packed_bytes += character.len_utf8();
                }
                assert_eq!(ranges.len(), minimum_chunks);
                let mut position = 0;
                for range in ranges {
                    assert_eq!(range.start_byte, position);
                    assert!((1..=1024).contains(&(range.end_byte - range.start_byte)));
                    assert!(range.text(&text).is_ok());
                    position = range.end_byte;
                }
                assert_eq!(position, text.len());
            }
        }
    }
}
