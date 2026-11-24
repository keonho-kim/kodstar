// Purpose: native 구문 근거의 겹친 구간을 병합하고 포함 관계를 판정.
// Pattern: Simple Module.
// Usage: evidence가 구간을 정규화하고 marker 선택기가 조회.
// Related: crates/kodstar-core/src/infrastructure/parse/input/native.rs, crates/kodstar-core/src/infrastructure/parse/input/native_evidence.rs.

use std::ops::Range;

pub(super) fn coalesce(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_unstable_by_key(|range| range.start);
    let mut output: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = output.last_mut()
            && previous.end >= range.start
        {
            previous.end = previous.end.max(range.end);
        } else {
            output.push(range);
        }
    }
    output
}

/// 구간은 시작 위치 순서로 정렬되어 있고 서로 겹치지 않아야 한다.
pub(super) fn contains(ranges: &[Range<usize>], offset: usize) -> bool {
    let index = ranges.partition_point(|range| range.start <= offset);
    index > 0 && ranges[index - 1].contains(&offset)
}
