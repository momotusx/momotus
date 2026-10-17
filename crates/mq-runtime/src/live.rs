//! Live view delivery: list diffs mirrored from eyeball-im, coalesced to at
//! most one batch per frame, plus whole-record snapshots.

use eyeball_im::VectorDiff;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use std::time::Duration;

/// Index-based list change, serializable for traces and every binding.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    feature = "ts",
    derive(ts_rs::TS),
    ts(export, export_to = "../../../apps/web/src/core/generated/")
)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ListDiff<T> {
    Append { values: Vec<T> },
    Clear,
    PushFront { value: T },
    PushBack { value: T },
    PopFront,
    PopBack,
    Insert { index: usize, value: T },
    Set { index: usize, value: T },
    Remove { index: usize },
    Truncate { length: usize },
    Reset { values: Vec<T> },
}

impl<T: Clone> From<VectorDiff<T>> for ListDiff<T> {
    fn from(diff: VectorDiff<T>) -> Self {
        match diff {
            VectorDiff::Append { values } => ListDiff::Append {
                values: values.into_iter().collect(),
            },
            VectorDiff::Clear => ListDiff::Clear,
            VectorDiff::PushFront { value } => ListDiff::PushFront { value },
            VectorDiff::PushBack { value } => ListDiff::PushBack { value },
            VectorDiff::PopFront => ListDiff::PopFront,
            VectorDiff::PopBack => ListDiff::PopBack,
            VectorDiff::Insert { index, value } => ListDiff::Insert { index, value },
            VectorDiff::Set { index, value } => ListDiff::Set { index, value },
            VectorDiff::Remove { index } => ListDiff::Remove { index },
            VectorDiff::Truncate { length } => ListDiff::Truncate { length },
            VectorDiff::Reset { values } => ListDiff::Reset {
                values: values.into_iter().collect(),
            },
        }
    }
}

impl<T: Clone> ListDiff<T> {
    /// Applies the diff to a plain vector, the way every UI binding does.
    pub fn apply(self, list: &mut Vec<T>) {
        match self {
            ListDiff::Append { values } => list.extend(values),
            ListDiff::Clear => list.clear(),
            ListDiff::PushFront { value } => list.insert(0, value),
            ListDiff::PushBack { value } => list.push(value),
            ListDiff::PopFront => {
                if !list.is_empty() {
                    list.remove(0);
                }
            }
            ListDiff::PopBack => {
                list.pop();
            }
            ListDiff::Insert { index, value } => list.insert(index, value),
            ListDiff::Set { index, value } => list[index] = value,
            ListDiff::Remove { index } => {
                list.remove(index);
            }
            ListDiff::Truncate { length } => list.truncate(length),
            ListDiff::Reset { values } => *list = values,
        }
    }
}

/// Frame budget for coalescing, about one 60 Hz frame.
pub const FRAME: Duration = Duration::from_millis(16);

/// Merges batches that arrive within one frame of the first, so a burst of
/// addon results reaches the UI as one update.
pub fn coalesce<S, T>(stream: S, window: Duration) -> impl Stream<Item = Vec<T>>
where
    S: Stream<Item = Vec<T>> + Unpin,
{
    futures_util::stream::unfold(stream, move |mut stream| async move {
        let mut batch = stream.next().await?;
        let deadline = tokio::time::sleep(window);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                next = stream.next() => match next {
                    Some(more) => batch.extend(more),
                    None => break,
                },
            }
        }
        Some((batch, stream))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eyeball_im::ObservableVector;

    #[tokio::test]
    async fn diffs_replayed_on_a_vec_match_the_source() {
        let mut source = ObservableVector::new();
        let (initial, stream) = source.subscribe().into_values_and_batched_stream();
        let mut mirror: Vec<i32> = initial.into_iter().collect();
        source.push_back(1);
        source.push_back(2);
        source.insert(1, 9);
        source.set(0, 5);
        source.remove(2);
        let mut batches = Box::pin(coalesce(stream, FRAME));
        let batch = batches.next().await.unwrap();
        for diff in batch {
            ListDiff::from(diff).apply(&mut mirror);
        }
        assert_eq!(mirror, vec![5, 9]);
        assert_eq!(source.iter().copied().collect::<Vec<_>>(), mirror);
    }
}
