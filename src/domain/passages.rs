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
            let mut start = paragraph_start;
            while start < end {
                let mut window_end = (start + PASSAGE_TARGET_BYTES).min(end);
                while !text.is_char_boundary(window_end) {
                    window_end -= 1;
                }
                emit(start, window_end)?;
                start = window_end;
            }
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
    fn golden_paragraph_packing_and_oversized_windows() {
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
            [(0, 702), (702, 1206), (1206, 2230), (2230, 2406)]
        );
        assert_eq!(prepare(&text, 3).unwrap_err().code(), "limit_exceeded");
        assert!(prepare("", 1).unwrap().is_empty());
        let text = format!("{}\n\n{}\n\n", "a".repeat(508), "b".repeat(512));
        assert_eq!(prepare(&text, 1).unwrap()[0].end_byte, 1024);
    }
}
