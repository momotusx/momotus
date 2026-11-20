//! Live view delivery: list diffs mirrored from eyeball-im, coalesced to at
//! most one batch per frame, plus whole-record snapshots.

use eyeball_im::VectorDiff;
use futures_util::{Stream, StreamExt};
use serde::Serialize;
use std::pin::Pin;
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
        let deadline = mq_rt::sleep(window);
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

/// A whole-snapshot view's pushes: at most one per `window`, carrying the
/// newest snapshot only. Every app keeps only the last snapshot of a push,
/// so older ones would be written and read for nothing (batch 13 perf C3).
/// A snapshot equal to the last one pushed is not pushed again: Search sent
/// four unchanged copies of 30 KB per typed letter, each one rebuilt by the
/// app (bench/perf-nav/core.jsonl, 2026-11-19).
pub fn snapshots<S, T>(updates: S, window: Duration) -> Pin<Box<dyn Stream<Item = Vec<T>> + Send>>
where
    S: Stream<Item = T> + Unpin + Send + 'static,
    T: Clone + PartialEq + Send + 'static,
{
    paced(updates, move |_| window)
}

/// `snapshots`, each push's window chosen by the first snapshot in it.
pub fn paced<S, T>(
    updates: S,
    window: impl Fn(&T) -> Duration + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Vec<T>> + Send>>
where
    S: Stream<Item = T> + Unpin + Send + 'static,
    T: Clone + PartialEq + Send + 'static,
{
    Box::pin(futures_util::stream::unfold(
        (updates, window, None::<T>),
        |(mut updates, window, mut last)| async move {
            loop {
                let mut newest = updates.next().await?;
                let deadline = mq_rt::sleep(window(&newest));
                tokio::pin!(deadline);
                loop {
                    tokio::select! {
                        _ = &mut deadline => break,
                        next = updates.next() => match next {
                            Some(snapshot) => newest = snapshot,
                            None => break,
                        },
                    }
                }
                if last.as_ref() != Some(&newest) {
                    last = Some(newest.clone());
                    return Some((vec![newest], (updates, window, last)));
                }
            }
        },
    ))
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

    #[tokio::test]
    async fn a_burst_of_snapshots_goes_out_as_its_newest() {
        let source = eyeball::SharedObservable::new(0);
        let mut pushes = snapshots(source.subscribe(), FRAME);
        let burst = async {
            for n in 1..=3 {
                source.set(n);
                mq_rt::sleep(Duration::from_millis(2)).await;
            }
        };
        let (push, ()) = tokio::join!(pushes.next(), burst);
        assert_eq!(push, Some(vec![3]), "one push, the newest only");
        source.set(4);
        assert_eq!(pushes.next().await, Some(vec![4]));
    }

    #[tokio::test]
    async fn a_push_waits_as_long_as_its_first_snapshot_asks() {
        let source = eyeball::SharedObservable::new(0);
        let slow = |n: &i32| {
            if *n >= 10 {
                Duration::from_millis(200)
            } else {
                FRAME
            }
        };
        let mut pushes = paced(source.subscribe(), slow);
        let later = async {
            source.set(10);
            mq_rt::sleep(Duration::from_millis(60)).await;
            source.set(11);
        };
        let (push, ()) = tokio::join!(pushes.next(), later);
        assert_eq!(push, Some(vec![11]), "the slow window took both");
    }

    #[tokio::test]
    async fn a_snapshot_equal_to_the_last_push_is_not_pushed() {
        let source = eyeball::SharedObservable::new(0);
        let pushes = snapshots(source.subscribe(), FRAME);
        // 1, then 1 again, then a burst that ends where it began, then 3.
        let sets = async {
            for burst in [&[1][..], &[1], &[2, 1], &[3]] {
                for &n in burst {
                    source.set(n);
                    mq_rt::sleep(Duration::from_millis(2)).await;
                }
                mq_rt::sleep(FRAME * 3).await;
            }
            drop(source);
        };
        let (got, ()) = tokio::join!(pushes.collect::<Vec<_>>(), sets);
        assert_eq!(
            got,
            [vec![1], vec![3]],
            "nothing on screen changed before 3"
        );
    }
}
