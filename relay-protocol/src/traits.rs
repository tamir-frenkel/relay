use std::collections::BTreeMap;
use std::fmt::Debug;

use uuid::Uuid;

use crate::annotated::{Annotated, MetaMap, MetaTree};
use crate::value::{Val, Value};

/// A value that can be empty.
pub trait Empty {
    /// Returns whether this value is empty.
    fn is_empty(&self) -> bool;

    /// Returns whether this value is empty or all of its descendants are empty.
    ///
    /// This only needs to be implemented for containers by calling `Empty::is_deep_empty` on all
    /// children. The default implementation calls `Empty::is_empty`.
    ///
    /// For containers of `Annotated` elements, this must call `Annotated::skip_serialization`.
    #[inline]
    fn is_deep_empty(&self) -> bool {
        self.is_empty()
    }
}

/// Defines behavior for skipping the serialization of fields.
///
/// This behavior is configured via the `skip_serialization` attribute on fields of structs. It is
/// passed as parameter to `ToValue::skip_serialization` of the corresponding field.
///
/// The default for fields in derived structs is `SkipSerialization::Null(true)`, which will skips
/// `null` values under the field recursively. Newtype structs (`MyType(T)`) and enums pass through
/// to their inner type and variant, respectively.
///
/// ## Example
///
/// ```ignore
/// #[derive(Debug, Empty, ToValue)]
/// struct Helper {
///     #[metastructure(skip_serialization = "never")]
///     items: Annotated<Array<String>>,
/// }
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SkipSerialization {
    /// Serialize all values. Missing values will be serialized as `null`.
    Never,

    /// Do not serialize `null` values but keep empty collections.
    ///
    /// If the `bool` flag is set to `true`, this applies to all descendants recursively; if it is
    /// set to `false`, this only applies to direct children and does not propagate down.
    Null(bool),

    /// Do not serialize empty objects as indicated by the `Empty` trait.
    ///
    /// If the `bool` flag is set to `true`, this applies to all descendants recursively; if it is
    /// set to `false`, this only applies to direct children and does not propagate down.
    Empty(bool),
}

impl SkipSerialization {
    /// Returns the serialization behavior for child elements.
    ///
    /// Shallow behaviors - `Null(false)` and `Empty(false)` - propagate as `Never`, all others
    /// remain the same. This allows empty containers to be skipped while their contents will
    /// serialize with `null` values.
    pub fn descend(self) -> Self {
        match self {
            SkipSerialization::Null(false) => SkipSerialization::Never,
            SkipSerialization::Empty(false) => SkipSerialization::Never,
            other => other,
        }
    }
}

impl Default for SkipSerialization {
    fn default() -> Self {
        SkipSerialization::Null(true)
    }
}

/// Implemented for all meta structures.
pub trait FromValue: Debug {
    /// Creates a meta structure from an annotated boxed value.
    fn from_value(value: Annotated<Value>) -> Annotated<Self>
    where
        Self: Sized;
}

/// Implemented for all meta structures.
pub trait IntoValue: Debug + Empty {
    /// Boxes the meta structure back into a value.
    fn into_value(self) -> Value
    where
        Self: Sized;

    /// Extracts children meta map out of a value.
    fn extract_child_meta(&self) -> MetaMap
    where
        Self: Sized,
    {
        MetaMap::new()
    }

    /// Efficiently serializes the payload directly.
    fn serialize_payload<S>(&self, s: S, behavior: SkipSerialization) -> Result<S::Ok, S::Error>
    where
        Self: Sized,
        S: serde::Serializer;

    /// Extracts the meta tree out of annotated value.
    ///
    /// This should not be overridden by implementators, instead `extract_child_meta`
    /// should be provided instead.
    fn extract_meta_tree(value: &Annotated<Self>) -> MetaTree
    where
        Self: Sized,
    {
        MetaTree {
            meta: value.1.clone(),
            children: match value.0 {
                Some(ref value) => IntoValue::extract_child_meta(value),
                None => BTreeMap::default(),
            },
        }
    }
}

/// A type that supports field access by paths.
///
/// This is the runtime version of [`get_value!`](crate::get_value!) and additionally supports
/// indexing into [`Value`]. For typed access to static paths, use the macros instead.
///
/// # Syntax
///
/// The path identifies a value within the structure. A path consists of components separated by
/// `.`, where each of the components is the name of a field to access. Every path starts with the
/// name of the root level component, which must match for this type.
///
/// Special characters are escaped with a `\`. The two special characters are:
///  - `\.` matches a literal dot in a path component.
///  - `\\` matches a literal backslash in a path component.
///
/// # Implementation
///
/// Implementation of the `Getter` trait should follow a set of conventions to ensure the paths
/// align with expectations based on the layout of the implementing type:
///
///  1. The name of the root component should be the lowercased version of the name of the
///     structure. For example, a structure called `Event` would use `event` as the root component.
///  2. All fields of the structure are referenced by the name of the field in the containing
///     structure. This also applies to mappings such as `HashMap`, where the key should be used as
///     field name. For recursive access, this translates transitively through the hierarchy.
///  3. Newtypes and structured enumerations do not show up in paths. This especially applies to
///     `Option`, which opaque in the path: `None` is simply propagated up.
///
/// In the future, a derive for the `Getter` trait will be added to simplify implementing the
/// `Getter` trait.
///
/// # Example
///
/// ```
/// use relay_protocol::{Getter, Val};
///
/// struct Root {
///     a: u64,
///     b: Nested,
/// }
///
/// struct Nested {
///     c: u64,
/// }
///
/// impl Getter for Root {
///     fn get_value(&self, path: &str) -> Option<Val<'_>> {
///         match path.strip_prefix("root.")? {
///             "a" => Some(self.a.into()),
///             "b.c" => Some(self.b.c.into()),
///             _ => None,
///         }
///     }
/// }
///
///
/// let root = Root {
///   a: 1,
///   b: Nested {
///     c: 2,
///   }
/// };
///
/// assert_eq!(root.get_value("root.a"), Some(Val::U64(1)));
/// assert_eq!(root.get_value("root.b.c"), Some(Val::U64(2)));
/// assert_eq!(root.get_value("root.d"), None);
/// ```
pub trait Getter {
    /// Returns the serialized value of a field pointed to by a `path`.
    fn get_value(&self, path: &str) -> Option<Val<'_>>; // deprecated
}

/// TODO(ja): Doc
///
/// # Implementation
///
/// - Either implement [`Getter2::as_val`].
/// - Or implement [`Getter2::get`] and [`Getter2::keys`]` together
///
/// It is also legal to implement both at the same time.
pub trait Getter2: AsGetter {
    // fn as_bool(&self) -> Option<bool> {
    //     None
    // }

    // fn as_i64(&self) -> Option<i64> {
    //     None
    // }

    // fn as_u64(&self) -> Option<u64> {
    //     None
    // }

    // fn as_f64(&self) -> Option<f64> {
    //     None
    // }

    // fn as_str(&self) -> Option<&str> {
    //     None
    // }

    // fn as_uuid(&self) -> Option<Uuid> {
    //     None
    // }

    // OR ----------------------------------------

    /// Returns the simple val representation of this instance.
    ///
    /// Defaults to `None`. This should normally not be implemented for structs with fields.
    fn as_val(&self) -> Option<Val<'_>> {
        None
    }

    // -------------------------------------------

    /// Returns a reference to the getter in the specified field.
    ///
    /// # Implementation
    ///
    /// Defaults to `None`. When implemented, [`Getter2::keys`] must be implemented too and return
    /// the same set of keys supported by this function.
    ///
    /// By convention, this should not be implemented for primitive values as they do not contain
    /// structured data.
    fn get(&self, key: &str) -> Option<&dyn Getter2> {
        None
    }

    /// Iterates the keys of all fields in this instance.
    ///
    /// # Implementation
    ///
    /// Defaults to [`IndexIter::empty`]. This should return all fields supported by
    /// [`Getter2::keys`].
    ///
    /// To implement this for a struct with known fields, use [`IndexIter::from_slice`] and pass a
    /// slice containing static field names.
    ///
    /// ```
    /// todo!("example")
    /// ```
    ///
    /// To implement
    ///
    /// ```
    /// todo!("example")
    /// ```
    ///
    /// By convention, this should not be implemented for primitive values as they do not contain
    /// structured data.
    fn keys(&self) -> IndexIter<'_> {
        IndexIter::empty()
    }

    /// Iterates all fields along with references to their getters.
    ///
    /// # Implementation
    ///
    /// This method is provided for all implementors and does not have to be implemented.
    fn iter(&self) -> Iter<'_> {
        Iter {
            getter: self.as_getter(),
            indexes: self.keys(),
        }
    }

    /// Resolves the getter at a specified path, if it exists.
    ///
    /// Returns `None` if either any field along the path does not exist, or one of the fields is
    /// `None`. To retrieve the value of the getter, use [`as_val`](Getter2::as_val) on the returned
    /// reference or call [`get_value`](Getter2::get_value) directly.
    fn get_path(&self, path: &str) -> Option<&dyn Getter2> {
        let mut current = self.as_getter();
        for component in path.split('.') {
            current = current.get(component)?;
        }
        Some(current)
    }

    /// Resolves the [`Val`] at the specified path, if it exists.
    ///
    /// Returns `None` if either any field along the path does not exist, one of the fields is
    /// `None`, or the getter at the target returns `None` for [`as_val`](Getter2::as_val). To
    /// retrieve a reference to the getter instead, call [`get_path`](Getter2::get_path).
    fn get_value(&self, path: &str) -> Option<Val<'_>> {
        self.get_path(path)?.as_val()
    }

    /// Iterates keys at the given path.
    ///
    /// This is a shorthand for calling `.get_path(path).keys()`.
    fn keys_at(&self, path: &str) -> IndexIter<'_> {
        match self.get_path(path) {
            Some(getter) => getter.keys(),
            None => IndexIter::empty(),
        }
    }

    /// Iterates keys and values at the given path.
    ///
    /// This is a shorthand for calling `.get_path(path).iter()`.
    fn iter_at(&self, path: &str) -> Iter<'_> {
        match self.get_path(path) {
            Some(getter) => getter.iter(),
            None => Iter::empty(),
        }
    }
}

impl Getter2 for () {}

impl Getter2 for bool {
    fn as_val(&self) -> Option<Val<'_>> {
        Some(Val::Bool(*self))
    }

    // #[inline]
    // fn as_bool(&self) -> Option<bool> {
    //     Some(*self)
    // }
}

impl Getter2 for i64 {
    // #[inline]
    // fn as_i64(&self) -> Option<i64> {
    //     Some(*self)
    // }

    // #[inline]
    // fn as_u64(&self) -> Option<u64> {
    //     todo!()
    // }

    // #[inline]
    // fn as_f64(&self) -> Option<f64> {
    //     todo!()
    // }

    fn as_val(&self) -> Option<Val<'_>> {
        Some(Val::I64(*self))
    }
}

impl Getter2 for u64 {
    // #[inline]
    // fn as_i64(&self) -> Option<i64> {
    //     todo!()
    // }

    // #[inline]
    // fn as_u64(&self) -> Option<u64> {
    //     Some(*self)
    // }

    // #[inline]
    // fn as_f64(&self) -> Option<f64> {
    //     todo!()
    // }

    fn as_val(&self) -> Option<Val<'_>> {
        Some(Val::U64(*self))
    }
}

impl Getter2 for f64 {
    // #[inline]
    // fn as_i64(&self) -> Option<i64> {
    //     todo!()
    // }

    // #[inline]
    // fn as_u64(&self) -> Option<u64> {
    //     todo!()
    // }

    // #[inline]
    // fn as_f64(&self) -> Option<f64> {
    //     Some(*self)
    // }

    fn as_val(&self) -> Option<Val<'_>> {
        Some(Val::F64(*self))
    }
}

impl Getter2 for String {
    // #[inline]
    // fn as_str(&self) -> Option<&str> {
    //     Some(self)
    // }

    fn as_val(&self) -> Option<Val<'_>> {
        Some(Val::String(self))
    }
}

impl Getter2 for std::borrow::Cow<'_, str> {
    // #[inline]
    // fn as_str(&self) -> Option<&str> {
    //     Some(self)
    // }

    fn as_val(&self) -> Option<Val<'_>> {
        Some(Val::String(self))
    }
}

impl Getter2 for Uuid {
    // #[inline]
    // fn as_uuid(&self) -> Option<Uuid> {
    //     Some(*self)
    // }

    fn as_val(&self) -> Option<Val<'_>> {
        Some(Val::Uuid(*self))
    }
}

impl Getter2 for Value {
    fn as_val(&self) -> Option<Val<'_>> {
        Some(self.into())
    }

    fn get(&self, key: &str) -> Option<&dyn Getter2> {
        match self {
            Value::Bool(_) => None,
            Value::I64(_) => None,
            Value::U64(_) => None,
            Value::F64(_) => None,
            Value::String(_) => None,
            Value::Array(_) => todo!("implement index for arrays"),
            Value::Object(object) => Getter2::get(object, key),
        }
    }

    fn keys(&self) -> IndexIter<'_> {
        match self {
            Value::Bool(_) => IndexIter::empty(),
            Value::I64(_) => IndexIter::empty(),
            Value::U64(_) => IndexIter::empty(),
            Value::F64(_) => IndexIter::empty(),
            Value::String(_) => IndexIter::empty(),
            Value::Array(_) => todo!(),
            Value::Object(object) => IndexIter::new(Getter2::keys(object)),
        }
    }
}

impl<T> Getter2 for Option<T>
where
    T: Getter2,
{
    #[inline]
    fn as_val(&self) -> Option<Val<'_>> {
        self.as_ref().and_then(Getter2::as_val)
    }

    #[inline]
    fn get(&self, key: &str) -> Option<&dyn Getter2> {
        self.as_ref().and_then(|getter| getter.get(key))
    }

    #[inline]
    fn keys(&self) -> IndexIter<'_> {
        self.as_ref().map(Getter2::keys).unwrap_or_default()
    }
}

impl<K, V> Getter2 for BTreeMap<K, V>
where
    K: std::borrow::Borrow<str> + Ord,
    V: Getter2,
{
    fn get(&self, key: &str) -> Option<&dyn Getter2> {
        self.get(key).to_getter()
    }

    fn keys(&self) -> IndexIter<'_> {
        IndexIter::new(self.keys().map(|k| k.borrow()))
    }
}

/// Helper trait that converts references to types implementing [`Getter2`] into a trait object.
///
/// This trait is automatically implemented by all types implementing [`Getter2`].
pub trait AsGetter {
    /// Returns the reference to the trait object.
    fn as_getter(&self) -> &dyn Getter2;
}

impl<T> AsGetter for T
where
    T: Getter2 + Sized,
{
    fn as_getter(&self) -> &dyn Getter2 {
        self
    }
}

/// Convenience helper for working with options of [`Getter2`].
pub trait GetterExt<'a> {
    /// Returns this type cast as optional trait object.
    fn to_getter(self) -> Option<&'a dyn Getter2>;
}

// impl<'a, T> GetterExt<'a> for &'a T
// where
//     T: Getter2,
// {
//     #[inline]
//     fn to_getter(self) -> Option<&'a dyn Getter2> {
//         Some(self as _)
//     }
// }

impl<'a, T> GetterExt<'a> for &'a Option<T>
where
    T: Getter2,
{
    #[inline]
    fn to_getter(self) -> Option<&'a dyn Getter2> {
        self.as_ref().to_getter()
    }
}

impl<'a, T> GetterExt<'a> for Option<&'a T>
where
    T: Getter2,
{
    #[inline]
    fn to_getter(self) -> Option<&'a dyn Getter2> {
        self.map(|v| v as _)
    }
}

enum IndexIterRepr<'a> {
    Slice(std::slice::Iter<'a, &'a str>),
    Dyn(Box<dyn Iterator<Item = &'a str> + 'a>),
}

/// Iterator over keys in a [`Getter2`], returned by [`keys`](Getter2::keys).
pub struct IndexIter<'a> {
    repr: IndexIterRepr<'a>,
}

impl Default for IndexIter<'_> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<'a> IndexIter<'a> {
    /// Creates an iterator that will not yield any values.
    pub fn empty() -> Self {
        Self::from_slice(&[])
    }

    /// Creates an iterator that yields from the provided `iter`.
    pub fn new<I>(iter: I) -> Self
    where
        I: Iterator<Item = &'a str> + 'a,
    {
        Self {
            repr: IndexIterRepr::Dyn(Box::new(iter)),
        }
    }

    /// Creates an iterator that yields the elements of the provided `slice`.
    pub fn from_slice(slice: &'a [&'a str]) -> Self {
        Self {
            repr: IndexIterRepr::Slice(slice.iter()),
        }
    }
}

impl<'a> Iterator for IndexIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self.repr {
            IndexIterRepr::Slice(ref mut inner) => inner.next().copied(),
            IndexIterRepr::Dyn(ref mut inner) => inner.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.repr {
            IndexIterRepr::Slice(ref inner) => inner.size_hint(),
            IndexIterRepr::Dyn(ref inner) => inner.size_hint(),
        }
    }

    fn count(self) -> usize
    where
        Self: Sized,
    {
        match self.repr {
            IndexIterRepr::Slice(inner) => inner.count(),
            IndexIterRepr::Dyn(inner) => inner.count(),
        }
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        match self.repr {
            IndexIterRepr::Slice(ref mut inner) => inner.nth(n).copied(),
            IndexIterRepr::Dyn(ref mut inner) => inner.nth(n),
        }
    }
}

/// Iterator over keys and references to their values in a [`Getter2`], returned by
/// [`iter`](Getter2::iter).
pub struct Iter<'a> {
    getter: &'a dyn Getter2,
    indexes: IndexIter<'a>,
}

impl Iter<'_> {
    fn empty() -> Self {
        Self {
            getter: &(),
            indexes: IndexIter::empty(),
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a str, Option<&'a dyn Getter2>);

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.indexes.next()?;
        Some((index, self.getter.get(index)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.indexes.size_hint()
    }

    fn count(self) -> usize
    where
        Self: Sized,
    {
        self.indexes.count()
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        let index = self.indexes.nth(n)?;
        Some((index, self.getter.get(index)))
    }
}

#[cfg(test)]
mod tests {
    use similar_asserts::assert_eq;

    use super::*;

    struct Outer {
        foo: Foo,
        bar: Bar,
    }

    impl Getter2 for Outer {
        fn get(&self, key: &str) -> Option<&dyn Getter2> {
            match key {
                "foo" => Some(&self.foo as _),
                "bar" => Some(&self.bar as _),
                _ => None,
            }
        }

        fn keys(&self) -> IndexIter<'_> {
            IndexIter::from_slice(&["foo", "bar"])
        }
    }

    struct Foo {
        value: String,
    }

    impl Getter2 for Foo {
        fn get(&self, key: &str) -> Option<&dyn Getter2> {
            match key {
                "value" => Some(&self.value as _),
                _ => None,
            }
        }

        fn keys(&self) -> IndexIter<'_> {
            IndexIter::from_slice(&["value"])
        }
    }

    struct Bar {
        value: u64,
    }

    impl Getter2 for Bar {
        fn get(&self, key: &str) -> Option<&dyn Getter2> {
            match key {
                "value" => Some(&self.value as _),
                _ => None,
            }
        }

        fn keys(&self) -> IndexIter<'_> {
            IndexIter::from_slice(&["value"])
        }
    }

    #[test]
    fn get() {
        let bar = Bar { value: 42 };
        let getter = Getter2::get(&bar, "value").unwrap();
        assert_eq!(getter.as_val().unwrap(), 42u64.into());
    }

    #[test]
    fn get_unknown() {
        let bar = Bar { value: 42 };
        assert!(Getter2::get(&bar, "unknown").is_none());
    }

    #[test]
    fn keys() {
        let foo = Outer {
            foo: Foo {
                value: "test".to_string(),
            },
            bar: Bar { value: 42 },
        };

        let keys: Vec<_> = Getter2::keys(&foo).collect();
        assert_eq!(keys, &["foo", "bar"]);
    }

    #[test]
    fn get_path() {
        let foo = Outer {
            foo: Foo {
                value: "test".to_string(),
            },
            bar: Bar { value: 42 },
        };

        let getter = Getter2::get_path(&foo, "foo.value").unwrap();
        assert_eq!(getter.as_val().unwrap(), "test".into());
    }

    #[test]
    fn get_value() {
        let foo = Outer {
            foo: Foo {
                value: "test".to_string(),
            },
            bar: Bar { value: 42 },
        };

        let val = Getter2::get_value(&foo, "foo.value").unwrap();
        assert_eq!(val, "test".into());
    }

    #[test]
    fn keys_at() {
        let foo = Outer {
            foo: Foo {
                value: "test".to_string(),
            },
            bar: Bar { value: 42 },
        };

        let keys: Vec<_> = Getter2::keys_at(&foo, "foo").collect();
        assert_eq!(keys, &["value"]);
    }
}
