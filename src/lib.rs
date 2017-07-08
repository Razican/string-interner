#![cfg_attr(all(feature = "bench", test), feature(test))]

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
//! 	use string_interner::StringInterner;
//! 	let mut interner = StringInterner::<usize>::new();
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

#[cfg(test)]
mod tests;

use std::vec;
use std::slice;
use std::iter;
use std::marker;

use std::hash::{Hash, Hasher, BuildHasher};
use std::collections::HashMap;
use std::collections::hash_map::RandomState;

/// Represents indices into the `StringInterner`.
/// 
/// Values of this type shall be lightweight as the whole purpose
/// of interning values is to be able to store them efficiently in memory.
/// 
/// This trait allows definitions of custom `Symbol`s besides
/// the already supported unsigned integer primitives.
pub trait Symbol: Copy + Ord + Eq {
	/// Creates a symbol explicitely from a usize primitive type.
	/// 
	/// Defaults to simply using the standard From<usize> trait.
	fn from_usize(val: usize) -> Self;

	/// Creates a usize explicitely from this symbol.
	/// 
	/// Defaults to simply using the standard Into<usize> trait.
	fn to_usize(self) -> usize;
}

impl<T> Symbol for T where T: Copy + Ord + Eq + From<usize> + Into<usize> {
	#[inline]
	fn from_usize(val: usize) -> Self { val.into() }
	#[inline]
	fn to_usize(self) -> usize { self.into() }
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
unsafe impl<Sym> Send for StringInterner<Sym> where Sym: Symbol + Send {}
unsafe impl<Sym> Sync for StringInterner<Sym> where Sym: Symbol + Sync {}


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

/// Defaults to using usize as the underlying and internal
/// symbol data representation within this `StringInterner`.
pub type DefaultStringInterner = StringInterner<usize>;

/// Provides a bidirectional mapping between String stored within
/// the interner and indices.
/// The main purpose is to store every unique String only once and
/// make it possible to reference it via lightweight indices.
/// 
/// Compilers often use this for implementing a symbol table.
/// 
/// The main goal of this `StringInterner` is to store String
/// with as low memory overhead as possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringInterner<Sym, H = RandomState>
	where Sym: Symbol,
	      H  : BuildHasher
{
	map   : HashMap<InternalStrRef, Sym, H>,
	values: Vec<Box<str>>
}

impl Default for StringInterner<usize, RandomState> {
	fn default() -> Self {
		StringInterner::new()
	}
}

impl<Sym> StringInterner<Sym>
	where Sym: Symbol
{
	/// Creates a new empty `StringInterner`.
	#[inline]
	pub fn new() -> StringInterner<Sym, RandomState> {
		StringInterner{
			map   : HashMap::new(),
			values: Vec::new()
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

impl<Sym, H> StringInterner<Sym, H>
	where Sym: Symbol,
	      H  : BuildHasher
{
	/// Creates a new empty `StringInterner` with the given hasher.
	#[inline]
	pub fn with_hasher(hash_builder: H) -> StringInterner<Sym, H> {
		StringInterner{
			map   : HashMap::with_hasher(hash_builder),
			values: Vec::new()
		}
	}

	/// Creates a new empty `StringInterner` with the given initial capacity and the given hasher.
	#[inline]
	pub fn with_capacity_and_hasher(cap: usize, hash_builder: H) -> StringInterner<Sym, H> {
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
	pub fn get_or_intern<T>(&mut self, val: T) -> Sym
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
	fn intern<T>(&mut self, new_val: T) -> Sym
		where T: Into<String> + AsRef<str>
	{
		let new_id : Sym            = self.make_symbol();
		let new_ref: InternalStrRef = new_val.as_ref().into();
		self.values.push(new_val.into().into_boxed_str());
		self.map.insert(new_ref, new_id);
		new_id
	}

	/// Creates a new symbol for the current state of the interner.
	fn make_symbol(&self) -> Sym {
		Sym::from_usize(self.len())
	}

	/// Returns a string slice to the string identified by the given symbol if available.
	/// Else, None is returned.
	#[inline]
	pub fn resolve(&self, symbol: Sym) -> Option<&str> {
		self.values
			.get(symbol.to_usize())
			.map(|boxed_str| boxed_str.as_ref())
	}

	/// Returns a string slice to the string identified by the given symbol,
	/// without doing bounds checking. So use it very carefully!
	#[inline]
	pub unsafe fn resolve_unchecked(&self, symbol: Sym) -> &str {
		self.values.get_unchecked(symbol.to_usize()).as_ref()
	}

	/// Returns the given string's symbol for this interner if existent.
	#[inline]
	pub fn get<T>(&self, val: T) -> Option<Sym>
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
	pub fn iter(&self) -> Iter<Sym> {
		Iter::new(self)
	}

	/// Returns an iterator over all intern indices and their associated strings.
	#[inline]
	pub fn iter_values(&self) -> Values<Sym> {
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
}

/// Iterator over the pairs of symbols and interned string for a `StringInterner`.
pub struct Iter<'a, Sym> {
	iter: iter::Enumerate<slice::Iter<'a, Box<str>>>,
	mark: marker::PhantomData<Sym>
}

impl<'a, Sym> Iter<'a, Sym>
	where Sym: Symbol + 'a
{
	/// Creates a new iterator for the given StringIterator over pairs of 
	/// symbols and their associated interned string.
	#[inline]
	fn new<H>(interner: &'a StringInterner<Sym, H>) -> Self
		where H  : BuildHasher
	{
		Iter{iter: interner.values.iter().enumerate(), mark: marker::PhantomData}
	}
}

impl<'a, Sym> Iterator for Iter<'a, Sym>
	where Sym: Symbol + 'a
{
	type Item = (Sym, &'a str);

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
pub struct Values<'a, Sym>
	where Sym: Symbol + 'a
{
	iter: slice::Iter<'a, Box<str>>,
	mark: marker::PhantomData<Sym>
}

impl<'a, Sym> Values<'a, Sym>
	where Sym: Symbol + 'a
{
	/// Creates a new iterator for the given StringIterator over its interned strings.
	#[inline]
	fn new<H>(interner: &'a StringInterner<Sym, H>) -> Self
		where H  : BuildHasher
	{
		Values{
			iter: interner.values.iter(),
			mark: marker::PhantomData
		}
	}
}

impl<'a, Sym> Iterator for Values<'a, Sym>
	where Sym: Symbol + 'a
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

impl<Sym, H> iter::IntoIterator for StringInterner<Sym, H>
	where Sym: Symbol,
	      H  : BuildHasher
{
	type Item = (Sym, String);
	type IntoIter = IntoIter<Sym>;

	fn into_iter(self) -> Self::IntoIter {
		IntoIter{iter: self.values.into_iter().enumerate(), mark: marker::PhantomData}
	}
}

/// Iterator over the pairs of symbols and associated interned string when 
/// morphing a `StringInterner` into an iterator.
pub struct IntoIter<Sym>
	where Sym: Symbol
{
	iter: iter::Enumerate<vec::IntoIter<Box<str>>>,
	mark: marker::PhantomData<Sym>
}

impl<Sym> Iterator for IntoIter<Sym>
	where Sym: Symbol
{
	type Item = (Sym, String);

	fn next(&mut self) -> Option<Self::Item> {
		self.iter.next().map(|(num, boxed_str)| (Sym::from_usize(num), boxed_str.into_string()))
	}

	#[inline]
	fn size_hint(&self) -> (usize, Option<usize>) {
		self.iter.size_hint()
	}
}

#[cfg(all(feature = "bench", test))]
mod bench {
	use super::*;
    use test::{Bencher, black_box};

	fn read_file_to_string(path: &str) -> String {
		use std::io::prelude::*;
		use std::fs::File;

		let mut f = File::open(path).expect("bench file not found");
		let mut s = String::new();

		f.read_to_string(&mut s).expect("encountered problems writing bench file to string");
		s
	}

	fn read_default_test() -> String {
		read_file_to_string("bench/input.txt")
	}

	fn empty_setup<'a>(input: &'a str) -> (Vec<&'a str>, DefaultStringInterner) {
		let lines = input.split_whitespace().collect::<Vec<&'a str>>();
		let interner = DefaultStringInterner::with_capacity(lines.len());
		(lines, interner)
	}

	fn filled_setup<'a>(input: &'a str) -> (Vec<usize>, DefaultStringInterner) {
		let (lines, mut interner) = empty_setup(&input);
		let symbols = lines.iter().map(|&line| interner.get_or_intern(line)).collect::<Vec<_>>();
		(symbols, interner)
	}

	#[bench]
	fn bench_get_or_intern_unique(bencher: &mut Bencher) {
		let input = read_default_test();
		let (lines, mut interner) = empty_setup(&input);
		bencher.iter(|| {
			for &line in lines.iter() {
				black_box(interner.get_or_intern(line));
			}
			interner.clear();
		});
	}

	#[bench]
	fn bench_resolve(bencher: &mut Bencher) {
		let input = read_default_test();
		let (symbols, interner) = filled_setup(&input);
		bencher.iter(|| {
			for &sym in symbols.iter() {
				black_box(interner.resolve(sym));
			}
		});
	}

	#[bench]
	fn bench_resolve_unchecked(bencher: &mut Bencher) {
		let input = read_default_test();
		let (symbols, interner) = filled_setup(&input);
		bencher.iter(|| {
			for &sym in symbols.iter() {
				unsafe{ black_box(interner.resolve_unchecked(sym)) };
			}
		});
	}

	#[bench]
	fn bench_iter(bencher: &mut Bencher) {
		let input = read_default_test();
		let (_, interner) = filled_setup(&input);
		bencher.iter(|| {
			for (sym, strref) in interner.iter() {
				black_box((sym, strref));
			}
		})
	}

	#[bench]
	fn bench_values_iter(bencher: &mut Bencher) {
		let input = read_default_test();
		let (_, interner) = filled_setup(&input);
		bencher.iter(|| {
			for strref in interner.iter_values() {
				black_box(strref);
			}
		})
	}

	/// Mainly needed to approximate the `into_iterator` test below.
	#[bench]
	fn bench_clone(bencher: &mut Bencher) {
		let input = read_default_test();
		let (_, interner) = filled_setup(&input);
		bencher.iter(|| {
			black_box(interner.clone());
		})
	}

	#[bench]
	fn bench_into_iterator(bencher: &mut Bencher) {
		let input = read_default_test();
		let (_, interner) = filled_setup(&input);
		bencher.iter(|| {
			for (sym, string) in interner.clone() {
				black_box((sym, string));
			}
		})
	}
}
