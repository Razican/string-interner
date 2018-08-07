#![cfg_attr(all(feature = "bench", test), feature(test))]
#![doc(html_root_url = "https://docs.rs/crate/string-interner/0.7.0")]
#![deny(missing_docs)]

//! A string interning data structure that was designed for minimal memory-overhead
//! and fast access to the underlying interned string contents.
//! 
//! Uses a similar interface as the string interner of the rust compiler.
//! 
//! Provides support to use all primitive types as symbols
//! 
//! Example usage:
//! 
//! ```
//! 	use string_interner::DefaultStringInterner;
//! 	let mut interner = DefaultStringInterner::default();
//! 	let name0 = interner.get_or_intern("Elephant");
//! 	let name1 = interner.get_or_intern("Tiger");
//! 	let name2 = interner.get_or_intern("Horse");
//! 	let name3 = interner.get_or_intern("Tiger");
//! 	let name4 = interner.get_or_intern("Tiger");
//! 	let name5 = interner.get_or_intern("Mouse");
//! 	let name6 = interner.get_or_intern("Horse");
//! 	let name7 = interner.get_or_intern("Tiger");
//! 	assert_eq!(name0, 0);
//! 	assert_eq!(name1, 1);
//! 	assert_eq!(name2, 2);
//! 	assert_eq!(name3, 1);
//! 	assert_eq!(name4, 1);
//! 	assert_eq!(name5, 3);
//! 	assert_eq!(name6, 2);
//! 	assert_eq!(name7, 1);
//! ```

#[cfg(all(feature = "bench", test))]
extern crate test;

#[cfg(all(feature = "bench", test))]
extern crate fnv;

#[cfg(feature = "serde_support")]
extern crate serde;

#[cfg(all(feature = "serde_support", test))]
extern crate serde_json;

#[cfg(test)]
mod tests;

#[cfg(all(feature = "bench", test))]
mod benches;

#[cfg(feature = "serde_support")]
mod serde_impl;

use std::{
	collections::{hash_map::RandomState, HashMap},
	hash::{BuildHasher, Hash, Hasher},
	iter, marker,
	num::NonZeroU32,
	slice, u32, vec,
};

/// Types implementing this trait are able to act as symbols for string interners.
///
/// Symbols are returned by `StringInterner::get_or_intern` and allow look-ups of the
/// original string contents with `StringInterner::resolve`.
///
/// # Note
///
/// Optimal symbols allow for efficient comparisons and have a small memory footprint.
pub trait Symbol: Copy + Ord + Eq {
	/// Creates a symbol from a `usize`.
	///
	/// # Note
	///
	/// Implementations panic if the operation cannot succeed.
	fn from_usize(val: usize) -> Self;

	/// Returns the `usize` representation of `self`.
	fn to_usize(self) -> usize;
}

/// Symbol type used by the `DefaultStringInterner`.
///
/// # Note
///
/// This special symbol type has a memory footprint of 32 bits
/// and allows for certain space optimizations such as using it within an option: `Option<Sym>`
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sym(NonZeroU32);

impl Symbol for Sym {
	/// Creates a `Sym` from the given `usize`.
	///
	/// # Panics
	///
	/// If the given `usize` is greater than `u32::MAX - 1`.
	fn from_usize(val: usize) -> Self {
		assert!(val < u32::MAX as usize);
		Sym(unsafe { NonZeroU32::new_unchecked((val + 1) as u32) })
	}

	fn to_usize(self) -> usize {
		(self.0.get() as usize) - 1
	}
}

impl Symbol for usize {
	fn from_usize(val: usize) -> Self {
		val
	}

	fn to_usize(self) -> usize {
		self
	}
}

/// Internal reference to str used only within the `StringInterner` itself
/// to encapsulate the unsafe behaviour of interor references.
#[derive(Debug, Copy, Clone, Eq)]
struct InternalStrRef(*const str);

impl InternalStrRef {
	/// Creates an InternalStrRef from a str.
	/// 
	/// This just wraps the str internally.
	fn from_str(val: &str) -> Self {
		InternalStrRef(val as *const str)
	}


	/// Reinterprets this InternalStrRef as a str.
	/// 
	/// This is "safe" as long as this InternalStrRef only
	/// refers to strs that outlive this instance or
	/// the instance that owns this InternalStrRef.
	/// This should hold true for `StringInterner`.
	/// 
	/// Does not allocate memory!
	fn as_str(&self) -> &str {
		unsafe{ &*self.0 }
	}
}

impl<T> From<T> for InternalStrRef
	where T: AsRef<str>
{
	fn from(val: T) -> Self {
		InternalStrRef::from_str(val.as_ref())
	}
}

impl Hash for InternalStrRef {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.as_str().hash(state)
	}
}

impl PartialEq for InternalStrRef {
	fn eq(&self, other: &InternalStrRef) -> bool {
		self.as_str() == other.as_str()
	}
}

/// `StringInterner` that uses `Sym` as its underlying symbol type.
pub type DefaultStringInterner = StringInterner<Sym>;

/// Provides a bidirectional mapping between String stored within
/// the interner and indices.
/// The main purpose is to store every unique String only once and
/// make it possible to reference it via lightweight indices.
/// 
/// Compilers often use this for implementing a symbol table.
/// 
/// The main goal of this `StringInterner` is to store String
/// with as low memory overhead as possible.
#[derive(Debug, Clone, Eq)]
pub struct StringInterner<S, H = RandomState>
where
	S: Symbol,
	H: BuildHasher,
{
	map: HashMap<InternalStrRef, S, H>,
	values: Vec<Box<str>>,
}

impl<S, H> PartialEq for StringInterner<S, H>
where
	S: Symbol,
	H: BuildHasher,
{
	fn eq(&self, rhs: &Self) -> bool {
		self.len() == rhs.len() && self.values == rhs.values
	}
}

impl Default for StringInterner<Sym, RandomState> {
	#[inline]
	fn default() -> Self {
		StringInterner::new()
	}
}

// About `Send` and `Sync` impls for `StringInterner`
// --------------------------------------------------
// 
// tl;dr: Automation of Send+Sync impl was prevented by `InternalStrRef`
// being an unsafe abstraction and thus prevented Send+Sync default derivation.
// 
// These implementations are safe due to the following reasons:
//  - `InternalStrRef` cannot be used outside `StringInterner`.
//  - Strings stored in `StringInterner` are not mutable.
//  - Iterator invalidation while growing the underlying `Vec<Box<str>>` is prevented by
//    using an additional indirection to store strings.
unsafe impl<S, H> Send for StringInterner<S, H>
where
	S: Symbol + Send,
	H: BuildHasher,
{
}
unsafe impl<S, H> Sync for StringInterner<S, H>
where
	S: Symbol + Sync,
	H: BuildHasher,
{
}

impl<S> StringInterner<S>
where
	S: Symbol,
{
	/// Creates a new empty `StringInterner`.
	#[inline]
	pub fn new() -> StringInterner<S, RandomState> {
		StringInterner {
			map: HashMap::new(),
			values: Vec::new(),
		}
	}

	/// Creates a new `StringInterner` with the given initial capacity.
	#[inline]
	pub fn with_capacity(cap: usize) -> Self {
		StringInterner{
			map   : HashMap::with_capacity(cap),
			values: Vec::with_capacity(cap)
		}
	}

}

impl<S, H> StringInterner<S, H>
where
	S: Symbol,
	H: BuildHasher,
{
	/// Creates a new empty `StringInterner` with the given hasher.
	#[inline]
	pub fn with_hasher(hash_builder: H) -> StringInterner<S, H> {
		StringInterner{
			map   : HashMap::with_hasher(hash_builder),
			values: Vec::new()
		}
	}

	/// Creates a new empty `StringInterner` with the given initial capacity and the given hasher.
	#[inline]
	pub fn with_capacity_and_hasher(cap: usize, hash_builder: H) -> StringInterner<S, H> {
		StringInterner{
			map   : HashMap::with_hasher(hash_builder),
			values: Vec::with_capacity(cap)
		}
	}

	/// Interns the given value.
	/// 
	/// Returns a symbol to access it within this interner.
	/// 
	/// This either copies the contents of the string (e.g. for str)
	/// or moves them into this interner (e.g. for String).
	#[inline]
	pub fn get_or_intern<T>(&mut self, val: T) -> S
		where T: Into<String> + AsRef<str>
	{
		match self.map.get(&val.as_ref().into()) {
			Some(&sym) => sym,
			None       => self.intern(val)
		}
	}

	/// Interns the given value and ignores collissions.
	/// 
	/// Returns a symbol to access it within this interner.
	fn intern<T>(&mut self, new_val: T) -> S
		where T: Into<String> + AsRef<str>
	{
		let new_id: S = self.make_symbol();
		let new_boxed_val = new_val.into().into_boxed_str();
		let new_ref: InternalStrRef = new_boxed_val.as_ref().into();
		self.values.push(new_boxed_val);
		self.map.insert(new_ref, new_id);
		new_id
	}

	/// Creates a new symbol for the current state of the interner.
	fn make_symbol(&self) -> S {
		S::from_usize(self.len())
	}

	/// Returns a string slice to the string identified by the given symbol if available.
	/// Else, None is returned.
	#[inline]
	pub fn resolve(&self, symbol: S) -> Option<&str> {
		self.values
			.get(symbol.to_usize())
			.map(|boxed_str| boxed_str.as_ref())
	}

	/// Returns a string slice to the string identified by the given symbol,
	/// without doing bounds checking. So use it very carefully!
	#[inline]
	pub unsafe fn resolve_unchecked(&self, symbol: S) -> &str {
		self.values.get_unchecked(symbol.to_usize()).as_ref()
	}

	/// Returns the given string's symbol for this interner if existent.
	#[inline]
	pub fn get<T>(&self, val: T) -> Option<S>
		where T: AsRef<str>
	{
		self.map
			.get(&val.as_ref().into())
			.cloned()
	}

	/// Returns the number of uniquely stored Strings interned within this interner.
	#[inline]
	pub fn len(&self) -> usize {
		self.values.len()
	}

	/// Returns true if the string interner internes no elements.
	#[inline]
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Returns an iterator over the interned strings.
	#[inline]
	pub fn iter(&self) -> Iter<S> {
		Iter::new(self)
	}

	/// Returns an iterator over all intern indices and their associated strings.
	#[inline]
	pub fn iter_values(&self) -> Values<S> {
		Values::new(self)
	}

	/// Removes all interned Strings from this interner.
	/// 
	/// This invalides all `Symbol` entities instantiated by it so far.
	#[inline]
	pub fn clear(&mut self) {
		self.map.clear();
		self.values.clear()
	}

	/// Shrinks the capacity of the interner as much as possible.
	pub fn shrink_to_fit(&mut self) {
		self.map.shrink_to_fit();
		self.values.shrink_to_fit();
	}
}

/// Iterator over the pairs of symbols and interned string for a `StringInterner`.
pub struct Iter<'a, S> {
	iter: iter::Enumerate<slice::Iter<'a, Box<str>>>,
	mark: marker::PhantomData<S>,
}

impl<'a, S> Iter<'a, S>
where
	S: Symbol + 'a,
{
	/// Creates a new iterator for the given StringIterator over pairs of 
	/// symbols and their associated interned string.
	#[inline]
	fn new<H>(interner: &'a StringInterner<S, H>) -> Self
		where H  : BuildHasher
	{
		Iter{iter: interner.values.iter().enumerate(), mark: marker::PhantomData}
	}
}

impl<'a, S> Iterator for Iter<'a, S>
where
	S: Symbol + 'a,
{
	type Item = (S, &'a str);

	#[inline]
	fn next(&mut self) -> Option<Self::Item> {
		self.iter.next().map(|(num, boxed_str)| (Sym::from_usize(num), boxed_str.as_ref()))
	}

	#[inline]
	fn size_hint(&self) -> (usize, Option<usize>) {
		self.iter.size_hint()
	}
}

/// Iterator over the interned strings for a `StringInterner`.
pub struct Values<'a, S>
where
	S: Symbol + 'a,
{
	iter: slice::Iter<'a, Box<str>>,
	mark: marker::PhantomData<S>,
}

impl<'a, S> Values<'a, S>
where
	S: Symbol + 'a,
{
	/// Creates a new iterator for the given StringIterator over its interned strings.
	#[inline]
	fn new<H>(interner: &'a StringInterner<S, H>) -> Self
		where H  : BuildHasher
	{
		Values{
			iter: interner.values.iter(),
			mark: marker::PhantomData
		}
	}
}

impl<'a, S> Iterator for Values<'a, S>
where
	S: Symbol + 'a,
{
	type Item = &'a str;

	#[inline]
	fn next(&mut self) -> Option<Self::Item> {
		self.iter.next().map(|boxed_str| boxed_str.as_ref())
	}

	#[inline]
	fn size_hint(&self) -> (usize, Option<usize>) {
		self.iter.size_hint()
	}
}

impl<S, H> iter::IntoIterator for StringInterner<S, H>
where
	S: Symbol,
	H: BuildHasher,
{
	type Item = (S, String);
	type IntoIter = IntoIter<S>;

	fn into_iter(self) -> Self::IntoIter {
		IntoIter{iter: self.values.into_iter().enumerate(), mark: marker::PhantomData}
	}
}

/// Iterator over the pairs of symbols and associated interned string when 
/// morphing a `StringInterner` into an iterator.
pub struct IntoIter<S>
where
	S: Symbol,
{
	iter: iter::Enumerate<vec::IntoIter<Box<str>>>,
	mark: marker::PhantomData<S>,
}

impl<S> Iterator for IntoIter<S>
where
	S: Symbol,
{
	type Item = (S, String);

	fn next(&mut self) -> Option<Self::Item> {
		self.iter.next().map(|(num, boxed_str)| (Sym::from_usize(num), boxed_str.into_string()))
	}

	#[inline]
	fn size_hint(&self) -> (usize, Option<usize>) {
		self.iter.size_hint()
	}
}
