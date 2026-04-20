use hyperindex_protocol::symbols::{ByteRange, LinePosition, SourceSpan};
use tree_sitter::{InputEdit, Point};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    line_starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    pub fn new(contents: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in contents.bytes().enumerate() {
            if byte == b'\n' && index + 1 <= contents.len() {
                line_starts.push(index + 1);
            }
        }
        Self {
            line_starts,
            len: contents.len(),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn byte_to_line_position(&self, byte_offset: usize) -> LinePosition {
        let clamped = byte_offset.min(self.len);
        let line_index = match self.line_starts.binary_search(&clamped) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts[line_index];
        LinePosition {
            line: (line_index + 1) as u32,
            column: (clamped - line_start + 1) as u32,
        }
    }

    pub fn line_column_to_byte(&self, line: u32, column: u32) -> Option<usize> {
        if line == 0 || column == 0 {
            return None;
        }
        let line_index = line as usize - 1;
        let line_start = *self.line_starts.get(line_index)?;
        let line_end = self
            .line_starts
            .get(line_index + 1)
            .copied()
            .unwrap_or(self.len);
        let byte = line_start + column as usize - 1;
        (byte <= line_end).then_some(byte)
    }

    pub fn byte_range_to_span(&self, start: usize, end: usize) -> SourceSpan {
        let clamped_start = start.min(self.len);
        let clamped_end = end.min(self.len);
        SourceSpan {
            start: self.byte_to_line_position(clamped_start),
            end: self.byte_to_line_position(clamped_end),
            bytes: ByteRange {
                start: clamped_start as u32,
                end: clamped_end as u32,
            },
        }
    }

    pub fn point_for_byte(&self, byte_offset: usize) -> Point {
        let position = self.byte_to_line_position(byte_offset);
        Point {
            row: position.line.saturating_sub(1) as usize,
            column: position.column.saturating_sub(1) as usize,
        }
    }

    pub fn edit_from(
        &self,
        old_contents: &str,
        new_contents: &str,
        new_index: &LineIndex,
    ) -> InputEdit {
        let old_bytes = old_contents.as_bytes();
        let new_bytes = new_contents.as_bytes();
        let prefix_len = shared_prefix_len(old_bytes, new_bytes);
        let suffix_len = shared_suffix_len(old_bytes, new_bytes, prefix_len);
        let old_end_byte = old_bytes.len().saturating_sub(suffix_len);
        let new_end_byte = new_bytes.len().saturating_sub(suffix_len);

        InputEdit {
            start_byte: prefix_len,
            old_end_byte,
            new_end_byte,
            start_position: self.point_for_byte(prefix_len),
            old_end_position: self.point_for_byte(old_end_byte),
            new_end_position: new_index.point_for_byte(new_end_byte),
        }
    }
}

fn shared_prefix_len(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(left_byte, right_byte)| left_byte == right_byte)
        .count()
}

fn shared_suffix_len(left: &[u8], right: &[u8], prefix_len: usize) -> usize {
    let mut suffix_len = 0;
    while suffix_len + prefix_len < left.len()
        && suffix_len + prefix_len < right.len()
        && left[left.len() - 1 - suffix_len] == right[right.len() - 1 - suffix_len]
    {
        suffix_len += 1;
    }
    suffix_len
}

#[cfg(test)]
mod tests {
    use super::LineIndex;

    #[test]
    fn converts_bytes_and_line_columns_both_ways() {
        let index = LineIndex::new("alpha\nbeta\n");
        assert_eq!(index.line_count(), 3);
        assert_eq!(index.byte_to_line_position(0).line, 1);
        assert_eq!(index.byte_to_line_position(0).column, 1);
        assert_eq!(index.byte_to_line_position(6).line, 2);
        assert_eq!(index.byte_to_line_position(6).column, 1);
        assert_eq!(index.line_column_to_byte(2, 3), Some(8));
    }

    #[test]
    fn computes_incremental_edit_window() {
        let old_contents = "export const value = 1;\n";
        let new_contents = "export const value = 42;\n";
        let old_index = LineIndex::new(old_contents);
        let new_index = LineIndex::new(new_contents);
        let edit = old_index.edit_from(old_contents, new_contents, &new_index);

        assert_eq!(edit.start_byte, 21);
        assert_eq!(edit.old_end_byte, 22);
        assert_eq!(edit.new_end_byte, 23);
        assert_eq!(edit.start_position.row, 0);
        assert_eq!(edit.start_position.column, 21);
    }
}
