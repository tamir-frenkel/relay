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
/// - Either implement one of as_bool, as_i64, as_u64, as_f64, as_str, or as_uuid
/// - Or implement get and keys together
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

    fn as_val(&self) -> Option<Val<'_>> {
        None
    }

    // -------------------------------------------

    fn get(&self, key: &str) -> Option<&dyn Getter2> {
        None
    }

    fn keys(&self) -> IndexIter<'_> {
        IndexIter::empty()
    }

    fn iter(&self) -> Iter<'_> {
        Iter {
            getter: Some(self.as_getter()),
            indexes: self.keys(),
        }
    }

    fn get_path(&self, path: &str) -> Option<&dyn Getter2> {
        let mut current = self.as_getter();
        for component in path.split('.') {
            current = current.get(component)?;
        }
        Some(current)
    }

    fn get_value(&self, path: &str) -> Option<Val<'_>> {
        self.get_path(path)?.as_val()
    }

    fn keys_at(&self, path: &str) -> IndexIter<'_> {
        match self.get_path(path) {
            Some(getter) => getter.keys(),
            None => IndexIter::empty(),
        }
    }

    fn iter_at(&self, path: &str) -> Iter<'_> {
        match self.get_path(path) {
            Some(getter) => getter.iter(),
            None => Iter::empty(),
        }
    }
}

// impl Getter2 for () {}

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

trait AsGetter {
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

pub trait GetterExt<'a> {
    fn as_getter(self) -> Option<&'a dyn Getter2>;
}

impl<'a, T> GetterExt<'a> for &'a Option<T>
where
    T: Getter2,
{
    #[inline]
    fn as_getter(self) -> Option<&'a dyn Getter2> {
        self.as_ref().as_getter()
    }
}

impl<'a, T> GetterExt<'a> for Option<&'a T>
where
    T: Getter2,
{
    #[inline]
    fn as_getter(self) -> Option<&'a dyn Getter2> {
        self.map(|v| v as _)
    }
}

enum IndexIterRepr<'a> {
    Slice(std::slice::Iter<'a, &'a str>),
    Dyn(Box<dyn Iterator<Item = &'a str> + 'a>),
}

pub struct IndexIter<'a> {
    repr: IndexIterRepr<'a>,
}

impl Default for IndexIter<'_> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<'a> IndexIter<'a> {
    pub fn empty() -> Self {
        Self::from_slice(&[])
    }

    pub fn new<I>(iter: I) -> Self
    where
        I: Iterator<Item = &'a str> + 'a,
    {
        Self {
            repr: IndexIterRepr::Dyn(Box::new(iter)),
        }
    }

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

pub struct Iter<'a> {
    getter: Option<&'a dyn Getter2>,
    indexes: IndexIter<'a>,
}

impl Iter<'_> {
    fn empty() -> Self {
        Self {
            getter: None,
            indexes: IndexIter::empty(),
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a str, Option<&'a dyn Getter2>);

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.indexes.next()?;
        Some((index, self.getter?.get(index)))
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
        Some((index, self.getter?.get(index)))
    }
}
