use std::sync::Arc;

use crate::{Bucket, BucketsView};

/// Partitions a list of buckets using the provided partition function.
///
/// This returns an iterator of bucket views all sharing the original allocation.
pub fn partition<K>(
    buckets: Vec<Bucket>,
    f: impl FnMut(&Bucket) -> K,
) -> impl Iterator<Item = (K, BucketsView<Arc<[Bucket]>>)>
where
    K: Ord,
{
    let mut buckets = buckets.into();
    let partitions = partition_by(Arc::get_mut(&mut buckets).unwrap(), f);

    let view = BucketsView::new(Arc::clone(&buckets));
    partitions.map(move |partition| {
        (
            partition.partition,
            view.clone().select(partition.start, partition.end),
        )
    })
}

/// Sorts the passed slice unstably and returns an iterator yielding continous slices of the
/// resulting partitions.
fn partition_by<T, K>(slice: &mut [T], f: impl FnMut(&T) -> K) -> Partitions<K>
where
    K: Ord,
{
    let mut indices: Vec<_> = slice
        .iter()
        .map(f)
        .enumerate()
        .map(|(i, k)| (k, i))
        .collect();
    indices.sort_unstable();

    for i in 0..slice.len() {
        let mut index = indices[i].1;
        while index < i {
            index = indices[index].1;
        }
        indices[i].1 = index;
        slice.swap(i, index);
    }

    Partitions::new(indices)
}

/// A single partition returned by [`Partitions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Partition<K> {
    /// The start index of the partition in the backing slice.
    pub start: usize,
    /// The end index of the partition in the backing slice.
    pub end: usize,
    /// The partition key of the slice.
    pub partition: K,
}

/// Iterator over all continous partitions in a sorted slice.
///
/// Returned from [`partition_by`].
struct Partitions<K> {
    indices: std::vec::IntoIter<(K, usize)>,
    length: usize,
    current: Option<K>,
    index: usize,
}

impl<K> Partitions<K> {
    fn new(indices: Vec<(K, usize)>) -> Self {
        let length = indices.len();
        let mut indices = indices.into_iter();
        let current = indices.next().map(|(k, _)| k);

        Self {
            indices,
            length,
            current,
            index: 0,
        }
    }

    fn next_from_indices(&mut self) -> Option<K> {
        self.index += 1;
        self.indices.next().map(|(k, _)| k)
    }
}

impl<K> Iterator for Partitions<K>
where
    K: Ord,
{
    type Item = Partition<K>;

    fn next(&mut self) -> Option<Self::Item> {
        let start_index = self.index;

        let current = self.current.take()?;

        while let Some(k) = self.next_from_indices() {
            if k != current {
                self.current = Some(k);
                return Some(Partition {
                    partition: current,
                    start: start_index,
                    end: self.index,
                });
            }
        }

        Some(Partition {
            partition: current,
            start: start_index,
            end: self.length,
        })
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_json_snapshot;
    use relay_common::time::UnixTimestamp;

    use super::*;

    fn buckets<T>(s: &[u8]) -> T
    where
        T: FromIterator<Bucket>,
    {
        let timestamp = UnixTimestamp::from_secs(5000);
        Bucket::parse_all(s, timestamp)
            .collect::<Result<T, _>>()
            .unwrap()
    }

    #[test]
    fn test_partition_buckets() {
        let buckets: Vec<_> = buckets(b"a:1|c\nb:2|c\nc:3|c\nb:4|c\nc:5|c\nb:6|c\n");

        let mut partitions = partition(buckets, |bucket| bucket.name.as_bytes()[9] as char);

        let (partition, view) = partitions.next().unwrap();
        assert_eq!(partition, 'a');
        assert_json_snapshot!(view, @r###"
        [
          {
            "timestamp": 5000,
            "width": 0,
            "name": "c:custom/a@none",
            "type": "c",
            "value": 1.0
          }
        ]
        "###);

        let (partition, view) = partitions.next().unwrap();
        assert_eq!(partition, 'b');
        assert_json_snapshot!(view, @r###"
        [
          {
            "timestamp": 5000,
            "width": 0,
            "name": "c:custom/b@none",
            "type": "c",
            "value": 2.0
          },
          {
            "timestamp": 5000,
            "width": 0,
            "name": "c:custom/b@none",
            "type": "c",
            "value": 4.0
          },
          {
            "timestamp": 5000,
            "width": 0,
            "name": "c:custom/b@none",
            "type": "c",
            "value": 6.0
          }
        ]
        "###);

        let (partition, view) = partitions.next().unwrap();
        assert_eq!(partition, 'c');
        assert_json_snapshot!(view, @r###"
        [
          {
            "timestamp": 5000,
            "width": 0,
            "name": "c:custom/c@none",
            "type": "c",
            "value": 3.0
          },
          {
            "timestamp": 5000,
            "width": 0,
            "name": "c:custom/c@none",
            "type": "c",
            "value": 5.0
          }
        ]
        "###);

        assert!(partitions.next().is_none());
    }

    #[test]
    fn test_parition_sort_order() {
        let mut data: Vec<i32> = (0..100).rev().collect();

        for (i, partition) in partition_by(&mut data, |v| *v).enumerate() {
            assert_eq!(partition.start, i);
            assert_eq!(partition.end, i + 1);
            assert_eq!(partition.partition, i as i32);
            assert_eq!(&data[partition.start..partition.end], &[i as i32]);
        }
    }

    #[test]
    fn test_parition_multiple() {
        let mut data = vec!["a", "b", "c", "b", "a", "b"];

        let mut partitions = partition_by(&mut data, |v| v.as_bytes()[0] - b'a');

        let partition = partitions.next().unwrap();
        assert_eq!(
            partition,
            Partition {
                start: 0,
                end: 2,
                partition: 0,
            }
        );
        assert_eq!(&data[partition.start..partition.end], &["a", "a"]);

        let partition = partitions.next().unwrap();
        assert_eq!(
            partition,
            Partition {
                start: 2,
                end: 5,
                partition: 1,
            }
        );
        assert_eq!(&data[partition.start..partition.end], &["b", "b", "b"]);

        let partition = partitions.next().unwrap();
        assert_eq!(
            partition,
            Partition {
                start: 5,
                end: 6,
                partition: 2,
            }
        );
        assert_eq!(&data[partition.start..partition.end], &["c"]);

        assert_eq!(partitions.next(), None);
    }

    #[test]
    fn test_parition_empty() {
        let mut data = Vec::<i32>::new();

        let mut partitions = partition_by(&mut data, |v| *v);
        assert_eq!(partitions.next(), None);
    }
}
