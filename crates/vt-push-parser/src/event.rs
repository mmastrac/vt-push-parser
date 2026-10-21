//! Event types.
use std::{io, iter::Map};

use crate::AsciiControl;

/// Helper function to format UTF-8 chunks with ASCII control character handling
fn fmt_utf8_bytes_with_ascii_control(
    f: &mut std::fmt::Formatter<'_>,
    bytes: &[u8],
) -> std::fmt::Result {
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if let Ok(c) = AsciiControl::try_from(c) {
                write!(f, "{c}")?;
            } else {
                write!(f, "{c}")?;
            }
        }
        if !chunk.invalid().is_empty() {
            write!(f, "<{}>", hex::encode(chunk.invalid()))?;
        }
    }
    Ok(())
}

/// Helper function to format UTF-8 chunks for parameters (simple formatting)
fn fmt_utf8_bytes_simple(f: &mut std::fmt::Formatter<'_>, bytes: &[u8]) -> std::fmt::Result {
    for chunk in bytes.utf8_chunks() {
        write!(f, "{}", chunk.valid())?;
        if !chunk.invalid().is_empty() {
            write!(f, "<{}>", hex::encode(chunk.invalid()))?;
        }
    }
    Ok(())
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VTIntermediate {
    pub(crate) data: [u8; 2],
}

impl VTIntermediate {
    pub const fn empty() -> Self {
        Self { data: [0, 0] }
    }

    pub const fn one(c: u8) -> Self {
        assert!(c >= 0x20 && c <= 0x2F);
        Self { data: [c, 0] }
    }

    pub const fn two(c1: u8, c2: u8) -> Self {
        assert!(c1 >= 0x20 && c1 <= 0x2F);
        assert!(c2 >= 0x20 && c2 <= 0x2F);
        Self { data: [c1, c2] }
    }

    pub fn has(&self, c: u8) -> bool {
        self.data[0] == c || self.data[1] == c
    }

    pub fn clear(&mut self) {
        self.data[0] = 0;
        self.data[1] = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.data[0] == 0 && self.data[1] == 0
    }

    pub fn len(&self) -> usize {
        self.data.iter().filter(|&&c| c != 0).count()
    }

    pub fn first(&self) -> Option<u8> {
        if self.data[0] != 0 {
            Some(self.data[0])
        } else {
            None
        }
    }

    pub fn second(&self) -> Option<u8> {
        if self.data[1] != 0 {
            Some(self.data[1])
        } else {
            None
        }
    }

    #[must_use]
    pub fn push(&mut self, c: u8) -> bool {
        if !(0x20..=0x2F).contains(&c) {
            return false;
        }

        // Invalid duplicate intermediate
        if self.data[0] == c {
            return false;
        }

        if self.data[0] == 0 {
            self.data[0] = c;
            true
        } else if self.data[1] == 0 {
            self.data[1] = c;
            true
        } else {
            false
        }
    }

    pub const fn const_eq(&self, other: &Self) -> bool {
        self.data[0] == other.data[0] && self.data[1] == other.data[1]
    }

    pub fn byte_len(&self) -> usize {
        self.data.iter().filter(|&&c| c != 0).count()
    }
}

impl AsRef<[u8]> for VTIntermediate {
    fn as_ref(&self) -> &[u8] {
        &self.data[..self.byte_len()]
    }
}

impl std::fmt::Debug for VTIntermediate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Inefficient
        write!(f, "'")?;
        for c in self.data.iter() {
            if *c == 0 {
                break;
            }
            write!(f, "{}", *c as char)?;
        }
        write!(f, "'")?;
        Ok(())
    }
}

/// A single parameter, as collected by the parser.
///
/// A fixed-capacity inline buffer of [`MAX_PARAM_LEN`] bytes.
#[derive(Clone, Copy)]
pub struct Param {
    data: [u8; MAX_PARAM_LEN],
    len: u8,
    /// Whether any byte was dropped (per-parameter overflow).
    overflowed: bool,
}

impl Param {
    /// An empty parameter. `const`-callable and usable in statics.
    pub const EMPTY: Self = Self {
        data: [0; MAX_PARAM_LEN],
        len: 0,
        overflowed: false,
    };

    /// Whether any byte was dropped because the parameter exceeded
    /// [`MAX_PARAM_LEN`] bytes.
    #[inline]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Append a byte, dropping bytes past [`MAX_PARAM_LEN`].
    #[inline]
    pub fn push(&mut self, b: u8) {
        if (self.len as usize) < MAX_PARAM_LEN {
            self.data[self.len as usize] = b;
            self.len += 1;
        } else {
            self.overflowed = true;
        }
    }

    /// Append the bytes of a slice, dropping bytes past [`MAX_PARAM_LEN`].
    #[inline]
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.push(b);
        }
    }

    /// Drop all bytes.
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
        self.overflowed = false;
    }

    /// The collected bytes.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }
}

impl std::ops::Deref for Param {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsRef<[u8]> for Param {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl PartialEq for Param {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Param {}

impl std::hash::Hash for Param {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state)
    }
}

impl std::fmt::Debug for Param {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

/// The maximum number of parameters collected for a single escape sequence.
pub const MAX_PARAMS: usize = 128;

/// The maximum number of bytes collected for a single parameter. This is the
/// same as the inline capacity of [`Param`].
pub const MAX_PARAM_LEN: usize = 32;

/// The maximum number of parameter bytes (excluding `;` separators) collected
/// for a single escape sequence.
pub const MAX_PARAM_BYTES: usize = 1024;

/// Fixed-capacity, inline parameter storage for the parser hot path.
///
/// Sequences may be split across `feed_with` calls, so parameters cannot be
/// borrowed from the input and must be buffered by the parser. This type does
/// so with a bounded, inline arena so that parsing is allocation-free (and
/// `memcpy`-only) for all input:
///
/// - at most [`MAX_PARAMS`] parameters per sequence;
/// - at most [`MAX_PARAM_LEN`] bytes per parameter;
/// - at most [`MAX_PARAM_BYTES`] parameter bytes in total per sequence.
#[derive(Clone, Copy)]
pub struct ParamList {
    /// Length of each collected parameter.
    lens: [u8; MAX_PARAMS],
    /// Concatenated parameter bytes.
    data: [u8; MAX_PARAM_BYTES],
    /// Number of collected parameters.
    count: u16,
    /// Number of used bytes in `data`.
    used: u16,
    /// Whether anything was dropped (per-parameter, or sequence-wide).
    overflowed: bool,
}

impl ParamList {
    /// An empty list. `const`-callable and usable in statics.
    pub const EMPTY: Self = Self {
        lens: [0; MAX_PARAMS],
        data: [0; MAX_PARAM_BYTES],
        count: 0,
        used: 0,
        overflowed: false,
    };

    /// The number of collected parameters.
    #[inline]
    pub fn len(&self) -> usize {
        self.count as usize
    }

    /// Whether no parameters have been collected.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The collected parameters, as byte slices.
    #[inline]
    pub fn iter(&self) -> ParamListIter<'_> {
        let (data, lens, count) = ParamSource::Inline(self).parts();
        ParamListIter {
            data,
            lens,
            count,
            index: 0,
            offset: 0,
        }
    }

    /// Whether any parameter bytes were dropped: bytes past a parameter's
    /// [`MAX_PARAM_LEN`] cap, or parameters past the [`MAX_PARAMS`] /
    /// [`MAX_PARAM_BYTES`] sequence caps.
    #[inline]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Record that something was dropped (crate-internal: the parser's
    /// current-parameter buffer clamps per byte, before `push_param`).
    #[inline]
    pub(crate) fn mark_overflowed(&mut self) {
        self.overflowed = true;
    }

    /// The parameter at `index`, if any.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&[u8]> {
        if index >= self.count as usize {
            return None;
        }
        let offset: usize = self.lens[..index].iter().map(|&len| len as usize).sum();
        let len = self.lens[index] as usize;
        Some(&self.data[offset..offset + len])
    }

    /// The number of parameter bytes (including `;` separators).
    #[inline]
    pub fn byte_len(&self) -> usize {
        self.used as usize + self.count.saturating_sub(1) as usize
    }

    /// Append a parameter, clamping on overflow (see the type docs).
    ///
    /// Bytes beyond [`MAX_PARAM_LEN`] within the current parameter, and any
    /// parameter that does not fit the remaining sequence capacity, are
    /// dropped; parsing continues normally. Sets
    /// [`overflowed`](Self::overflowed) if anything was dropped.
    #[inline]
    pub fn push_param(&mut self, param: &[u8]) {
        if param.len() > MAX_PARAM_LEN {
            self.overflowed = true;
        }
        if self.count as usize >= MAX_PARAMS || self.used as usize >= MAX_PARAM_BYTES {
            self.overflowed = true;
            return;
        }
        let remaining = MAX_PARAM_BYTES - self.used as usize;
        let len = param.len().min(MAX_PARAM_LEN).min(remaining);
        if len < param.len() {
            self.overflowed = true;
        }
        let start = self.used as usize;
        self.data[start..start + len].copy_from_slice(&param[..len]);
        self.lens[self.count as usize] = len as u8;
        self.count += 1;
        self.used += len as u16;
    }

    /// Drop all collected parameters.
    #[inline]
    pub fn clear(&mut self) {
        self.count = 0;
        self.used = 0;
        self.overflowed = false;
    }
}

impl PartialEq for ParamList {
    fn eq(&self, other: &Self) -> bool {
        // `data`/`lens` may contain stale bytes past `count`/`used`, so compare
        // the logical contents only.
        self.count == other.count && self.iter().eq(other.iter())
    }
}

impl Eq for ParamList {}

impl std::hash::Hash for ParamList {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the logical contents only, mirroring `PartialEq`: the raw
        // arrays may contain stale bytes past `count`/`used`.
        self.count.hash(state);
        for param in self.iter() {
            param.hash(state);
        }
    }
}

impl Default for ParamList {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl std::fmt::Debug for ParamList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ParamList {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.count as usize))?;
        for param in self.iter() {
            seq.serialize_element(param)?;
        }
        seq.end()
    }
}

static EMPTY_PARAMS: ParamList = ParamList::EMPTY;

/// The parameter storage backing a [`ParamBuf`]: either the parser's inline
/// [`ParamList`] or the content-sized heap storage of a [`ParamBufOwned`].
#[derive(Copy, Clone)]
pub(crate) enum ParamSource<'a> {
    Inline(&'a ParamList),
    Owned(&'a ParamBufOwned),
}

impl<'a> ParamSource<'a> {
    /// The concatenated parameter bytes, the per-parameter lengths and the
    /// parameter count.
    #[inline]
    fn parts(self) -> (&'a [u8], &'a [u8], usize) {
        match self {
            ParamSource::Inline(list) => (
                &list.data[..list.used as usize],
                &list.lens[..list.count as usize],
                list.count as usize,
            ),
            ParamSource::Owned(owned) => (
                owned.data.as_deref().unwrap_or(&[]),
                owned.lens.as_deref().unwrap_or(&[]),
                owned.lens.as_ref().map_or(0, |lens| lens.len()),
            ),
        }
    }

    #[inline]
    fn len(self) -> usize {
        self.parts().2
    }

    #[inline]
    fn is_empty(self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn overflowed(self) -> bool {
        match self {
            ParamSource::Inline(list) => list.overflowed(),
            ParamSource::Owned(owned) => owned.overflowed,
        }
    }

    fn get(self, index: usize) -> Option<&'a [u8]> {
        let (data, lens, count) = self.parts();
        if index >= count {
            return None;
        }
        let offset: usize = lens[..index].iter().map(|&len| len as usize).sum();
        let len = lens[index] as usize;
        Some(&data[offset..offset + len])
    }

    #[inline]
    fn byte_len(self) -> usize {
        let (data, _, count) = self.parts();
        data.len() + count.saturating_sub(1)
    }

    #[inline]
    fn iter(self) -> ParamListIter<'a> {
        let (data, lens, count) = self.parts();
        ParamListIter {
            data,
            lens,
            count,
            index: 0,
            offset: 0,
        }
    }
}

impl PartialEq for ParamSource<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl Eq for ParamSource<'_> {}

impl std::hash::Hash for ParamSource<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Mirror `PartialEq`: hash the logical contents only.
        self.len().hash(state);
        for param in self.iter() {
            param.hash(state);
        }
    }
}

impl std::fmt::Debug for ParamSource<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for ParamSource<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for param in self.iter() {
            seq.serialize_element(param)?;
        }
        seq.end()
    }
}

/// The parameters of an escape sequence (CSI/DCS), as emitted by the parser.
///
/// Parameters are buffered by the parser because sequences may be split
/// across `feed_with` calls. Storage is fixed-capacity and inline, which
/// bounds what can be reported for extreme input:
///
/// - at most [`MAX_PARAMS`] parameters;
/// - at most [`MAX_PARAM_LEN`] bytes per parameter;
/// - at most [`MAX_PARAM_BYTES`] parameter bytes (separators excluded);
///
/// A `ParamBuf` either borrows the parser's inline storage or the
/// content-sized storage of a [`ParamBufOwned`] (see [`ParamBufOwned::borrow`]);
/// the limits above apply to both.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ParamBuf<'a> {
    pub(crate) params: ParamSource<'a>,
}

impl<'a> IntoIterator for ParamBuf<'a> {
    type Item = &'a [u8];
    type IntoIter = std::iter::Map<ParamListIter<'a>, fn(&'a [u8]) -> &'a [u8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| p as &[u8])
    }
}

impl<'a> IntoIterator for &ParamBuf<'a> {
    type Item = &'a [u8];
    type IntoIter = std::iter::Map<ParamListIter<'a>, fn(&'a [u8]) -> &'a [u8]>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| p as &[u8])
    }
}

impl<'a> ParamBuf<'a> {
    pub const fn empty() -> Self {
        ParamBuf {
            params: ParamSource::Inline(&EMPTY_PARAMS),
        }
    }

    /// A view over the parser's inline parameter storage.
    pub(crate) const fn from_param_list(list: &'a ParamList) -> Self {
        ParamBuf {
            params: ParamSource::Inline(list),
        }
    }

    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&[u8]> {
        self.params.get(index)
    }

    pub fn try_parse<T: std::str::FromStr>(&self, index: usize) -> Option<T> {
        self.params.get(index).and_then(|p| {
            std::str::from_utf8(p)
                .ok()
                .and_then(|s| s.parse::<T>().ok())
        })
    }

    pub fn to_owned(&self) -> ParamBufOwned {
        match self.params {
            ParamSource::Inline(list) => ParamBufOwned::from_param_list(list),
            ParamSource::Owned(owned) => owned.clone(),
        }
    }

    /// Whether any parameter bytes were dropped by the capacity limits; see
    /// the [`ParamBuf`] documentation.
    pub fn overflowed(&self) -> bool {
        self.params.overflowed()
    }

    pub fn byte_len(&self) -> usize {
        self.params.byte_len()
    }

    pub fn numeric(&self) -> NumericParamBuf<'a> {
        NumericParamBuf {
            params: self.params,
        }
    }
}

/// Iterator over the parameters of a [`ParamList`] (and thus of [`ParamBuf`]
/// and [`ParamBufOwned`]).
///
/// This type is nameable so that a matching iterator can be written against
/// [`ParamBuf`] in downstream signatures; use `ParamList::iter`,
/// `ParamBuf::into_iter` or `&ParamBufOwned::into_iter` to construct one.
pub struct ParamListIter<'a> {
    /// Concatenated parameter bytes.
    data: &'a [u8],
    /// Length of each parameter.
    lens: &'a [u8],
    /// Number of parameters.
    count: usize,
    index: usize,
    offset: usize,
}

impl<'a> Iterator for ParamListIter<'a> {
    type Item = &'a [u8];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let len = self.lens[self.index] as usize;
        let param = &self.data[self.offset..self.offset + len];
        self.index += 1;
        self.offset += len;
        Some(param)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.count - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ParamListIter<'_> {}

impl std::iter::FusedIterator for ParamListIter<'_> {}

/// A view into a [`Param`] that contains only numeric parameters.
#[derive(Debug, Copy, Clone, Default)]
pub struct NumericParam<'a> {
    pub(crate) param: &'a [u8],
}

impl<'a> NumericParam<'a> {
    /// Try to parse the parameter as a single numeric value.
    pub fn sole(&self) -> Option<u16> {
        if !self.param.is_empty() && !self.param.contains(&b':') {
            if let Ok(s) = std::str::from_utf8(self.param) {
                s.parse::<u16>().ok()
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Try to parse the first parameter as a single numeric value.
    pub fn first(&self) -> Option<u16> {
        self.into_iter().next().flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.param.is_empty()
    }

    pub fn len(&self) -> usize {
        self.param.iter().filter(|p| **p == b':').count() + 1
    }

    /// Try to write the parameters to the given buffer, returning the written
    /// slice if successful. If any parameters are not a single-numeric value,
    /// writes a zero in that position.
    ///
    /// If the slice is not long enough, returns an error with the required
    /// length.
    pub fn try_write<'b>(&self, buf: &'b mut [u16]) -> Result<&'b [u16], usize> {
        let len = self.len();
        if buf.len() < len {
            return Err(buf.len());
        }
        for (i, param) in self.into_iter().enumerate() {
            buf[i] = param.unwrap_or(0);
        }
        Ok(&buf[..len])
    }
}

impl<'a> IntoIterator for NumericParam<'a> {
    type Item = Option<u16>;
    type IntoIter = Map<std::slice::Split<'a, u8, fn(&u8) -> bool>, fn(&'a [u8]) -> Option<u16>>;
    fn into_iter(self) -> Self::IntoIter {
        let fn1: fn(&u8) -> bool = |c: &u8| *c == b':';
        self.param.split(fn1).map(|p| {
            if p.is_empty() {
                None
            } else {
                std::str::from_utf8(p)
                    .ok()
                    .and_then(|s| s.parse::<u16>().ok())
            }
        })
    }
}

/// A view into a [`ParamBuf`] that contains only numeric parameters.
///
/// Each parameter may contain zero or more numeric values, separated by colons.
/// Empty parameters are interpreted as `None`.
#[derive(Debug, Copy, Clone)]
pub struct NumericParamBuf<'a> {
    pub(crate) params: ParamSource<'a>,
}

impl<'a> IntoIterator for NumericParamBuf<'a> {
    type Item = NumericParam<'a>;
    type IntoIter = std::iter::Map<ParamListIter<'a>, fn(&'a [u8]) -> NumericParam<'a>>;
    fn into_iter(self) -> Self::IntoIter {
        self.params.iter().map(|p| NumericParam { param: p })
    }
}

impl<'a> NumericParamBuf<'a> {
    pub const fn empty() -> Self {
        NumericParamBuf {
            params: ParamSource::Inline(&EMPTY_PARAMS),
        }
    }

    /// Try to write the parameters to the given buffer, returning the written
    /// slice if successful. If any parameters are not a single-numeric value,
    /// writes a zero in that position.
    ///
    /// If the slice is not long enough, returns an error with the required
    /// length.
    pub fn try_write<'b>(&self, buf: &'b mut [u16]) -> Result<&'b [u16], usize> {
        let len = self.params.len();
        if buf.len() < len {
            return Err(buf.len());
        }
        for (i, param) in self.into_iter().enumerate() {
            buf[i] = param.sole().unwrap_or(0);
        }
        Ok(&buf[..len])
    }

    pub fn get(&self, index: usize) -> Option<NumericParam<'a>> {
        self.params.get(index).map(|p| NumericParam { param: p })
    }

    pub fn first(&self) -> Option<NumericParam<'a>> {
        self.into_iter().next()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn len(&self) -> usize {
        self.params.len()
    }
}

/// A union of all possible events that can be emitted by the parser, with
/// borrowed data.
///
/// ## Streamed payloads
///
/// `Raw`, `DcsData`/`DcsEnd` and `OscData`/`OscEnd` borrow from the input of the
/// `feed_with` call that produced them, and the parser never copies payload
/// bytes. An event can therefore only ever cover bytes from a single call: a
/// payload that spans several calls arrives as several events.
///
/// For OSC and DCS strings, everything the parser has buffered when the string
/// terminates is emitted in the `OscEnd`/`DcsEnd` event; bytes flushed earlier
/// because a `feed_with` call ended (or because a DEL interrupted the run)
/// arrived as `OscData`/`DcsData` before it. In other words:
///
/// - the **payload** of a string is the concatenation of its `OscData` bytes
///   (or `DcsData` bytes) and the final `OscEnd.data` (or `DcsEnd`) bytes, in
///   event order;
/// - `OscEnd.data` / `DcsEnd` may be **empty**, including for a string with a
///   payload, whenever the last buffered byte was already flushed;
/// - feeding the same input in one call or in several may distribute the same
///   payload differently between the `*Data` and the `*End` events. The bytes
///   and their order do not change.
///
/// Callers that want the whole payload should accumulate it; the split is not
/// part of the input, only of the feeding.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum VTEvent<'a> {
    // Plain printable text from GROUND (coalesced)
    Raw(&'a [u8]),

    // C0 control (EXECUTE)
    C0(u8),

    // ESC final (with intermediates)
    Esc(Esc),

    // Invalid escape sequence
    EscInvalid(EscInvalid),

    // SS2
    Ss2(SS2),

    // SS3
    Ss3(SS3),

    // CSI short escape
    Csi(CSI<'a>),

    // DCS stream
    DcsStart(DCS<'a>),
    /// A run of DCS payload bytes, flushed before the end of the string. See
    /// the type-level docs: the payload is `DcsData` bytes followed by
    /// [`VTEvent::DcsEnd`] bytes.
    DcsData(&'a [u8]),
    /// The end of a DCS string. Carries any payload bytes still buffered, which
    /// may be empty.
    DcsEnd(&'a [u8]),
    DcsCancel,

    // OSC stream
    OscStart,
    /// A run of OSC payload bytes, flushed before the end of the string. See
    /// the type-level docs: the payload is `OscData` bytes followed by
    /// [`VTEvent::OscEnd`] data.
    OscData(&'a [u8]),
    OscEnd {
        /// Any payload bytes still buffered at the end of the string. May be
        /// empty, even for a non-empty payload; see the type-level docs.
        data: &'a [u8],
        /// Whether the BEL was used to end the OSC stream.
        used_bel: bool,
    },
    OscCancel,
}

impl<'a> std::fmt::Debug for VTEvent<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use VTEvent::*;
        match self {
            Raw(s) => {
                write!(f, "Raw('")?;
                fmt_utf8_bytes_with_ascii_control(f, s)?;
                write!(f, "')")?;
                Ok(())
            }
            EscInvalid(esc_invalid) => esc_invalid.fmt(f),
            C0(b) => write!(f, "C0({b:02x})"),
            Esc(esc) => esc.fmt(f),
            Ss2(ss2) => ss2.fmt(f),
            Ss3(ss3) => ss3.fmt(f),
            Csi(csi) => csi.fmt(f),
            DcsStart(dcs_start) => dcs_start.fmt(f),
            DcsData(s) | DcsEnd(s) => {
                if matches!(self, DcsEnd(..)) {
                    write!(f, "DcsEnd('")?;
                } else {
                    write!(f, "DcsData('")?;
                }
                fmt_utf8_bytes_with_ascii_control(f, s)?;
                write!(f, "')")?;
                Ok(())
            }
            DcsCancel => write!(f, "DcsCancel"),
            OscStart => write!(f, "OscStart"),
            OscData(s) | OscEnd { data: s, .. } => {
                if matches!(self, OscEnd { .. }) {
                    write!(f, "OscEnd('")?;
                } else {
                    write!(f, "OscData('")?;
                }
                fmt_utf8_bytes_with_ascii_control(f, s)?;
                write!(f, "')")?;
                Ok(())
            }
            OscCancel => write!(f, "OscCancel"),
        }
    }
}

impl<'a> VTEvent<'a> {
    pub fn csi(&self) -> Option<CSI<'a>> {
        match self {
            VTEvent::Csi(csi) => Some(CSI {
                private: csi.private,
                params: csi.params,
                intermediates: csi.intermediates,
                final_byte: csi.final_byte,
            }),
            _ => None,
        }
    }

    pub fn byte_len(&self) -> usize {
        use VTEvent::*;

        match self {
            Raw(s) => s.len(),
            C0(_) => 1,
            Esc(esc) => esc.intermediates.len() + 2 + esc.private.is_some() as usize,
            EscInvalid(esc_invalid) => {
                use self::EscInvalid::*;
                match esc_invalid {
                    One(..) => 2,
                    Two(..) => 3,
                    Three(..) => 4,
                    Four(..) => 5,
                }
            }
            Ss2(_) => 3,
            Ss3(_) => 3,
            Csi(csi) => {
                csi.private.is_some() as usize
                    + csi.params.byte_len()
                    + csi.intermediates.byte_len()
                    + 3
            }
            DcsStart(dcs_start) => {
                dcs_start.private.is_some() as usize
                    + dcs_start.params.byte_len()
                    + dcs_start.intermediates.byte_len()
                    + 3
            }
            DcsData(s) => s.len(),
            DcsEnd(s) => s.len() + 2,
            DcsCancel => 1,
            OscStart => 2,
            OscData(s) => s.len(),
            OscEnd { data, used_bel } => {
                if *used_bel {
                    data.len() + 1
                } else {
                    data.len() + 2
                }
            }
            OscCancel => 1,
        }
    }

    /// Encode the event into the provided buffer, returning the number of bytes
    /// required for the escape sequence in either `Ok(n)` or `Err(n)`.
    ///
    /// Note that some events may have multiple possible encodings, so this method
    /// may decide to choose whichever is more efficient.
    pub fn encode(&self, mut buf: &mut [u8]) -> Result<usize, usize> {
        use crate::{BEL, CAN, CSI, DCS, ESC, OSC, SS2, SS3, ST_FINAL};
        use VTEvent::*;

        let len = self.byte_len();

        if len > buf.len() {
            return Err(len);
        }

        match self {
            Raw(s) | OscData(s) | DcsData(s) => {
                buf[..s.len()].copy_from_slice(s);
            }
            EscInvalid(esc_invalid) => {
                use self::EscInvalid::*;
                buf[0] = ESC;
                match esc_invalid {
                    One(b) => buf[1] = *b,
                    Two(b1, b2) => {
                        buf[1] = *b1;
                        buf[2] = *b2;
                    }
                    Three(b1, b2, b3) => {
                        buf[1] = *b1;
                        buf[2] = *b2;
                        buf[3] = *b3;
                    }
                    Four(b1, b2, b3, b4) => {
                        buf[1] = *b1;
                        buf[2] = *b2;
                        buf[3] = *b3;
                        buf[4] = *b4;
                    }
                }
            }
            OscCancel | DcsCancel => {
                buf[0] = CAN;
            }
            C0(b) => {
                buf[0] = *b;
            }
            Ss2(ss2) => {
                buf[0] = ESC;
                buf[1] = SS2;
                buf[2] = ss2.char;
            }
            Ss3(ss3) => {
                buf[0] = ESC;
                buf[1] = SS3;
                buf[2] = ss3.char;
            }
            Esc(esc) => {
                buf[0] = ESC;
                if let Some(p) = esc.private {
                    buf[1] = p;
                    buf = &mut buf[1..];
                }
                buf[1..esc.intermediates.len() + 1]
                    .copy_from_slice(&esc.intermediates.data[..esc.intermediates.len()]);
                buf[esc.intermediates.len() + 1] = esc.final_byte;
            }
            Csi(csi) => {
                buf[0] = ESC;
                buf[1] = CSI;
                buf = &mut buf[2..];
                if let Some(p) = csi.private {
                    buf[0] = p;
                    buf = &mut buf[1..];
                }
                let mut params = csi.params.into_iter();
                if let Some(param) = params.next() {
                    buf[..param.len()].copy_from_slice(param);
                    buf = &mut buf[param.len()..];
                    for param in params {
                        buf[0] = b';';
                        buf = &mut buf[1..];
                        buf[..param.len()].copy_from_slice(param);
                        buf = &mut buf[param.len()..];
                    }
                }
                buf[..csi.intermediates.len()]
                    .copy_from_slice(&csi.intermediates.data[..csi.intermediates.len()]);
                buf[csi.intermediates.len()] = csi.final_byte;
            }
            DcsStart(dcs_start) => {
                buf[0] = ESC;
                buf[1] = DCS;
                buf = &mut buf[2..];
                if let Some(p) = dcs_start.private {
                    buf[0] = p;
                    buf = &mut buf[1..];
                }
                let mut params = dcs_start.params.into_iter();
                if let Some(param) = params.next() {
                    buf[..param.len()].copy_from_slice(param);
                    buf = &mut buf[param.len()..];
                    for param in params {
                        buf[0] = b';';
                        buf = &mut buf[1..];
                        buf[..param.len()].copy_from_slice(param);
                        buf = &mut buf[param.len()..];
                    }
                }
                buf[..dcs_start.intermediates.len()].copy_from_slice(
                    &dcs_start.intermediates.data[..dcs_start.intermediates.len()],
                );
                buf[dcs_start.intermediates.len()] = dcs_start.final_byte;
            }
            DcsEnd(data) => {
                buf[..data.len()].copy_from_slice(data);
                buf = &mut buf[data.len()..];
                buf[0] = ESC;
                buf[1] = ST_FINAL;
            }
            OscStart => {
                buf[0] = ESC;
                buf[1] = OSC;
            }
            OscEnd { data, used_bel } => {
                buf[..data.len()].copy_from_slice(data);
                buf = &mut buf[data.len()..];
                if *used_bel {
                    buf[0] = BEL;
                } else {
                    buf[0] = ESC;
                    buf[1] = ST_FINAL
                }
            }
        }

        Ok(len)
    }

    /// Encode the event into the provided writer, returning the number of bytes
    /// written.
    ///
    /// If the write fails for any reason, it is not possible to determine how
    /// many bytes were written.
    ///
    /// The order of writes is not specified and may change between versions of
    /// this crate.
    pub fn write_to(&self, mut writer: impl io::Write) -> Result<usize, io::Error> {
        use crate::{BEL, CAN, CSI, DCS, ESC, OSC, SS2, SS3, ST_FINAL};

        use VTEvent::*;
        let mut len = 0;
        match self {
            Raw(s) | OscData(s) | DcsData(s) => {
                writer.write_all(s)?;
                len = s.len();
            }
            EscInvalid(esc_invalid) => {
                use self::EscInvalid::*;
                match esc_invalid {
                    One(b) => {
                        writer.write_all(&[ESC, *b])?;
                        len = 2;
                    }
                    Two(b1, b2) => {
                        writer.write_all(&[ESC, *b1, *b2])?;
                        len = 3;
                    }
                    Three(b1, b2, b3) => {
                        writer.write_all(&[ESC, *b1, *b2, *b3])?;
                        len = 4;
                    }
                    Four(b1, b2, b3, b4) => {
                        writer.write_all(&[ESC, *b1, *b2, *b3, *b4])?;
                        len = 5;
                    }
                }
            }
            OscCancel | DcsCancel => {
                writer.write_all(&[CAN])?;
                len = 1;
            }
            C0(b) => {
                writer.write_all(&[*b])?;
                len = 1;
            }
            Ss2(ss2) => {
                writer.write_all(&[ESC, SS2, ss2.char])?;
                len = 3;
            }
            Ss3(ss3) => {
                writer.write_all(&[ESC, SS3, ss3.char])?;
                len = 3;
            }
            Esc(esc) => {
                writer.write_all(&[ESC])?;
                len = 1;
                if let Some(p) = esc.private {
                    writer.write_all(&[p])?;
                    len = 2;
                }
                if !esc.intermediates.is_empty() {
                    writer.write_all(&esc.intermediates.data[..esc.intermediates.len()])?;
                    len += esc.intermediates.len();
                }
                writer.write_all(&[esc.final_byte])?;
                len += 1;
            }
            Csi(csi) => {
                writer.write_all(&[ESC, CSI])?;
                len = 2;
                if let Some(p) = csi.private {
                    writer.write_all(&[p])?;
                    len = 3;
                }
                let mut first = true;
                for param in csi.params {
                    if !first {
                        writer.write_all(b";")?;
                        len += 1;
                    } else {
                        first = false;
                    }
                    writer.write_all(param)?;
                    len += param.len();
                }
                if !csi.intermediates.is_empty() {
                    writer.write_all(&csi.intermediates.data[..csi.intermediates.len()])?;
                    len += csi.intermediates.len();
                }
                writer.write_all(&[csi.final_byte])?;
                len += 1;
            }
            DcsStart(dcs_start) => {
                writer.write_all(&[ESC, DCS])?;
                len = 2;
                if let Some(p) = dcs_start.private {
                    writer.write_all(&[p])?;
                    len = 3;
                }
                let mut first = true;
                for param in dcs_start.params {
                    if !first {
                        writer.write_all(b";")?;
                        len += 1;
                    } else {
                        first = false;
                    }
                    writer.write_all(param)?;
                    len += param.len();
                }
                if !dcs_start.intermediates.is_empty() {
                    writer.write_all(
                        &dcs_start.intermediates.data[..dcs_start.intermediates.len()],
                    )?;
                    len += dcs_start.intermediates.len();
                }
                writer.write_all(&[dcs_start.final_byte])?;
                len += 1;
            }
            DcsEnd(s) => {
                writer.write_all(s)?;
                len += s.len();
                writer.write_all(&[ESC, ST_FINAL])?;
                len += 2;
            }
            OscStart => {
                writer.write_all(&[ESC, OSC])?;
                len = 2;
            }
            OscEnd { data, used_bel } => {
                writer.write_all(data)?;
                if *used_bel {
                    writer.write_all(&[BEL])?;
                    len = 1 + data.len();
                } else {
                    writer.write_all(&[ESC, ST_FINAL])?;
                    len = 2 + data.len();
                }
            }
        }
        Ok(len)
    }

    pub fn to_owned(&self) -> VTOwnedEvent {
        use VTEvent::*;
        match self {
            Raw(s) => VTOwnedEvent::Raw(s.to_vec()),
            C0(b) => VTOwnedEvent::C0(*b),
            Esc(esc) => VTOwnedEvent::Esc(*esc),
            EscInvalid(esc_invalid) => VTOwnedEvent::EscInvalid(*esc_invalid),
            Ss2(ss2) => VTOwnedEvent::Ss2(*ss2),
            Ss3(ss3) => VTOwnedEvent::Ss3(*ss3),
            Csi(csi) => VTOwnedEvent::Csi(CSIOwned {
                private: csi.private,
                params: csi.params.to_owned(),
                intermediates: csi.intermediates,
                final_byte: csi.final_byte,
            }),
            DcsStart(dcs_start) => VTOwnedEvent::DcsStart(DCSOwned {
                private: dcs_start.private,
                params: dcs_start.params.to_owned(),
                intermediates: dcs_start.intermediates,
                final_byte: dcs_start.final_byte,
            }),
            DcsData(s) => VTOwnedEvent::DcsData(s.to_vec()),
            DcsEnd(s) => VTOwnedEvent::DcsEnd(s.to_vec()),
            DcsCancel => VTOwnedEvent::DcsCancel,
            OscStart => VTOwnedEvent::OscStart,
            OscData(s) => VTOwnedEvent::OscData(s.to_vec()),
            OscEnd { data, used_bel } => VTOwnedEvent::OscEnd {
                data: data.to_vec(),
                used_bel: *used_bel,
            },
            OscCancel => VTOwnedEvent::OscCancel,
        }
    }
}

/// Owned parameters of an escape sequence, as stored on [`VTOwnedEvent`].
///
/// This is the owned counterpart of [`ParamBuf`]. Unlike the parser's
/// fixed-capacity inline storage (see [`ParamBuf`]), it is sized to its
/// contents on the heap, so an owned copy of a small parameter list is
/// small; the limits documented on [`ParamBuf`] apply as before. It is a
/// convenience type: like other `to_owned()` data, it is intended for use
/// outside the parsing hot path.
///
/// Holding no parameters is allocation-free.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ParamBufOwned {
    pub(crate) data: Option<Box<[u8]>>,
    pub(crate) lens: Option<Box<[u8]>>,
    pub(crate) overflowed: bool,
}

#[cfg(feature = "serde")]
impl serde::Serialize for ParamBufOwned {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("params", &ParamSource::Owned(self))?;
        map.end()
    }
}

impl ParamBufOwned {
    /// An owned copy of the logical contents of a [`ParamList`].
    pub(crate) fn from_param_list(list: &ParamList) -> Self {
        if list.count == 0 {
            return Self::EMPTY;
        }
        let data = &list.data[..list.used as usize];
        let lens = &list.lens[..list.count as usize];
        Self {
            data: Some(data.to_vec().into_boxed_slice()),
            lens: Some(lens.to_vec().into_boxed_slice()),
            overflowed: list.overflowed,
        }
    }

    /// Empty `ParamBufOwned` with no allocation.
    pub const EMPTY: Self = Self {
        data: None,
        lens: None,
        overflowed: false,
    };

    /// Create an empty `ParamBufOwned`.
    pub const fn empty() -> Self {
        Self::EMPTY
    }

    /// Create a `ParamBufOwned` from a slice of slices.
    ///
    /// Parameters beyond the [`ParamBuf`] capacity limits are clamped; see the
    /// [`ParamBuf`] documentation for the exact semantics.
    pub fn new(params: &[&[u8]]) -> Self {
        let mut list = ParamList::EMPTY;
        for param in params {
            list.push_param(param);
        }
        Self::from_param_list(&list)
    }

    pub fn len(&self) -> usize {
        self.lens.as_ref().map_or(0, |lens| lens.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<&[u8]> {
        ParamSource::Owned(self).get(index)
    }

    pub fn try_parse<T: std::str::FromStr>(&self, index: usize) -> Option<T> {
        self.get(index).and_then(|p| {
            std::str::from_utf8(p)
                .ok()
                .and_then(|s| s.parse::<T>().ok())
        })
    }

    /// Whether any parameter bytes were dropped by the capacity limits; see
    /// the [`ParamBuf`] documentation.
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// The number of parameter bytes (including `;` separators).
    pub fn byte_len(&self) -> usize {
        ParamSource::Owned(self).byte_len()
    }

    pub fn borrow(&self) -> ParamBuf<'_> {
        ParamBuf {
            params: ParamSource::Owned(self),
        }
    }

    pub fn numeric(&self) -> NumericParamBuf<'_> {
        NumericParamBuf {
            params: ParamSource::Owned(self),
        }
    }
}

/// An owning iterator over the parameters of a [`ParamBufOwned`].
pub struct ParamBufOwnedIter {
    data: Box<[u8]>,
    lens: Box<[u8]>,
    index: usize,
    offset: usize,
}

impl Iterator for ParamBufOwnedIter {
    type Item = Param;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.lens.len() {
            return None;
        }
        let len = self.lens[self.index] as usize;
        let mut param = Param::EMPTY;
        param.extend_from_slice(&self.data[self.offset..self.offset + len]);
        self.index += 1;
        self.offset += len;
        Some(param)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.lens.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ParamBufOwnedIter {}

impl std::iter::FusedIterator for ParamBufOwnedIter {}

impl IntoIterator for ParamBufOwned {
    type Item = Param;
    type IntoIter = ParamBufOwnedIter;
    fn into_iter(self) -> Self::IntoIter {
        ParamBufOwnedIter {
            data: self.data.unwrap_or_default(),
            lens: self.lens.unwrap_or_default(),
            index: 0,
            offset: 0,
        }
    }
}

impl<'b> IntoIterator for &'b ParamBufOwned {
    type Item = &'b [u8];
    type IntoIter = ParamListIter<'b>;
    fn into_iter(self) -> Self::IntoIter {
        ParamSource::Owned(self).iter()
    }
}

/// A union of all possible events that can be emitted by the parser, with owned
/// data.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq)]
pub enum VTOwnedEvent {
    Raw(Vec<u8>),
    C0(u8),
    Esc(Esc),
    EscInvalid(EscInvalid),
    Ss2(SS2),
    Ss3(SS3),
    Csi(CSIOwned),
    DcsStart(DCSOwned),
    DcsData(Vec<u8>),
    DcsEnd(Vec<u8>),
    DcsCancel,
    OscStart,
    OscData(Vec<u8>),
    OscEnd { data: Vec<u8>, used_bel: bool },
    OscCancel,
}

impl std::fmt::Debug for VTOwnedEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.borrow().fmt(f)
    }
}

impl VTOwnedEvent {
    pub fn borrow(&self) -> VTEvent<'_> {
        match self {
            VTOwnedEvent::Raw(s) => VTEvent::Raw(s),
            VTOwnedEvent::C0(b) => VTEvent::C0(*b),
            VTOwnedEvent::Esc(esc) => VTEvent::Esc(*esc),
            VTOwnedEvent::EscInvalid(esc_invalid) => VTEvent::EscInvalid(*esc_invalid),
            VTOwnedEvent::Ss2(ss2) => VTEvent::Ss2(SS2 { char: ss2.char }),
            VTOwnedEvent::Ss3(ss3) => VTEvent::Ss3(SS3 { char: ss3.char }),
            VTOwnedEvent::Csi(csi) => VTEvent::Csi(CSI {
                private: csi.private,
                params: csi.params.borrow(),
                intermediates: csi.intermediates,
                final_byte: csi.final_byte,
            }),
            VTOwnedEvent::DcsStart(dcs_start) => VTEvent::DcsStart(DCS {
                private: dcs_start.private,
                params: dcs_start.params.borrow(),
                intermediates: dcs_start.intermediates,
                final_byte: dcs_start.final_byte,
            }),
            VTOwnedEvent::DcsData(s) => VTEvent::DcsData(s),
            VTOwnedEvent::DcsEnd(s) => VTEvent::DcsEnd(s),
            VTOwnedEvent::DcsCancel => VTEvent::DcsCancel,
            VTOwnedEvent::OscStart => VTEvent::OscStart,
            VTOwnedEvent::OscData(s) => VTEvent::OscData(s),
            VTOwnedEvent::OscEnd { data, used_bel } => VTEvent::OscEnd {
                data,
                used_bel: *used_bel,
            },
            VTOwnedEvent::OscCancel => VTEvent::OscCancel,
        }
    }
}

/// An invalid escape sequence.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum EscInvalid {
    One(u8),
    Two(u8, u8),
    Three(u8, u8, u8),
    Four(u8, u8, u8, u8),
}

impl std::fmt::Debug for EscInvalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EscInvalid::One(b) => write!(f, "EscInvalid(1B {b:02X})")?,
            EscInvalid::Two(b1, b2) => write!(f, "EscInvalid(1B {b1:02X} {b2:02X})")?,
            EscInvalid::Three(b1, b2, b3) => {
                write!(f, "EscInvalid(1B {b1:02X} {b2:02X} {b3:02X})")?
            }
            EscInvalid::Four(b1, b2, b3, b4) => {
                write!(f, "EscInvalid(1B {b1:02X} {b2:02X} {b3:02X} {b4:02X})")?
            }
        }
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Esc {
    pub intermediates: VTIntermediate,
    pub private: Option<u8>,
    pub final_byte: u8,
}

impl std::fmt::Debug for Esc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Esc(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}, ", p as char)?;
        }
        write!(f, "{:?}, ", self.intermediates)?;
        if let Ok(c) = AsciiControl::try_from(self.final_byte as char) {
            write!(f, "{c})")?;
        } else {
            write!(f, "{})", self.final_byte as char)?;
        }
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SS2 {
    pub char: u8,
}

impl std::fmt::Debug for SS2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ss2(")?;
        if let Ok(c) = AsciiControl::try_from(self.char as char) {
            write!(f, "{c})")?;
        } else {
            write!(f, "{:?})", self.char as char)?;
        }
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SS3 {
    pub char: u8,
}

impl std::fmt::Debug for SS3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ss3(")?;
        if let Ok(c) = AsciiControl::try_from(self.char as char) {
            write!(f, "{c})")?;
        } else {
            write!(f, "{:?})", self.char as char)?;
        }
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CSI<'a> {
    pub private: Option<u8>,
    pub params: ParamBuf<'a>,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
}

impl<'a> std::fmt::Debug for CSI<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Csi(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}, ", p as char)?;
        }
        for param in &self.params {
            write!(f, "'")?;
            fmt_utf8_bytes_simple(f, param)?;
            write!(f, "', ")?;
        }
        write!(f, "{:?}, ", self.intermediates)?;
        write!(f, "{:?})", self.final_byte as char)?;
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DCS<'a> {
    pub private: Option<u8>,
    pub params: ParamBuf<'a>,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
}

impl<'a> std::fmt::Debug for DCS<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DcsStart(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}, ", p as char)?;
        }
        for param in &self.params {
            write!(f, "'")?;
            fmt_utf8_bytes_simple(f, param)?;
            write!(f, "', ")?;
        }
        write!(f, "{:?}, ", self.intermediates)?;
        write!(f, "{})", self.final_byte as char)?;
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq)]
pub struct CSIOwned {
    pub private: Option<u8>,
    pub params: ParamBufOwned,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
}

impl std::fmt::Debug for CSIOwned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Csi(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}", p as char)?;
        }
        for param in &self.params {
            write!(f, ", '")?;
            fmt_utf8_bytes_simple(f, param)?;
            write!(f, "'")?;
        }
        write!(f, ", {:?}", self.intermediates)?;
        write!(f, ", {:?})", self.final_byte as char)?;
        Ok(())
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, PartialEq, Eq)]
pub struct DCSOwned {
    pub private: Option<u8>,
    pub params: ParamBufOwned,
    pub intermediates: VTIntermediate,
    pub final_byte: u8,
}

impl std::fmt::Debug for DCSOwned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DcsStart(")?;
        if let Some(p) = self.private {
            write!(f, "{:?}", p as char)?;
        }
        for param in &self.params {
            write!(f, ", '")?;
            fmt_utf8_bytes_simple(f, param)?;
            write!(f, "'")?;
        }
        write!(f, ", {:?}", self.intermediates)?;
        write!(f, ", {})", self.final_byte as char)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_numeric_param_buf() {
        let param_buf = ParamBufOwned::new(&[b"1:2:3", b"4", b":"]);
        let numeric_param_buf = param_buf.numeric();
        assert_eq!(
            numeric_param_buf.into_iter().flatten().collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3), Some(4), None, None]
        );

        assert_eq!(numeric_param_buf.first().unwrap().sole(), None);
        assert_eq!(numeric_param_buf.first().unwrap().first(), Some(1));
        assert_eq!(numeric_param_buf.get(1).unwrap().sole(), Some(4));
        assert_eq!(numeric_param_buf.get(1).unwrap().first(), Some(4));
        assert_eq!(numeric_param_buf.get(2).unwrap().sole(), None);
        assert_eq!(numeric_param_buf.get(2).unwrap().first(), None);

        assert_eq!(
            numeric_param_buf
                .try_write(&mut [0, 0, 0, 0, 0, 0])
                .unwrap(),
            &[0, 4, 0]
        );
        assert_eq!(
            numeric_param_buf
                .get(0)
                .unwrap()
                .try_write(&mut [0, 0, 0, 0, 0, 0])
                .unwrap(),
            &[1, 2, 3]
        );
    }

    #[test]
    fn param_buf_owned_is_content_sized() {
        // No parameters: no heap storage at all.
        let empty = ParamBufOwned::new(&[]);
        assert!(empty.data.is_none());
        assert!(empty.lens.is_none());
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        // A small parameter list is stored in small heap allocations: the
        // bytes actually used ("1234" + "5"), not the parser's full inline
        // arena (MAX_PARAMS x MAX_PARAM_LEN).
        let small = ParamBufOwned::new(&[b"1234", b"5"]);
        assert_eq!(small.data.as_deref(), Some(&b"12345"[..]));
        assert_eq!(small.lens.as_deref(), Some(&[4, 1][..]));
        assert_eq!(small.len(), 2);
        assert_eq!(small.byte_len(), 6); // 5 bytes + 1 separator
        assert!(!small.overflowed());

        // Storing many parameters still only stores what was collected.
        let strings: Vec<String> = (0..MAX_PARAMS as u32).map(|i| i.to_string()).collect();
        let slices: Vec<&[u8]> = strings.iter().map(|s| s.as_bytes()).collect();
        let many = ParamBufOwned::new(&slices);
        assert_eq!(many.len(), MAX_PARAMS);
        assert_eq!(many.get(0), Some(&b"0"[..]));
        assert_eq!(many.get(127), Some(&b"127"[..]));
        assert_eq!(many.byte_len(), 10 + 90 * 2 + 28 * 3 + 127);
    }

    #[test]
    fn param_buf_owned_clamps_like_param_buf() {
        // Clamping applies to `ParamBufOwned::new` exactly as it does to the
        // parser: bytes past MAX_PARAM_LEN are dropped, params past the
        // sequence caps are dropped, and `overflowed()` reports both.
        let long = ParamBufOwned::new(&[&[b'7'; MAX_PARAM_LEN + 8][..]]);
        assert_eq!(long.len(), 1);
        assert_eq!(long.get(0).map(<[u8]>::len), Some(MAX_PARAM_LEN));
        assert!(long.overflowed());

        let too_many_strings: Vec<String> = (0..(MAX_PARAMS + 8) as u32)
            .map(|i| i.to_string())
            .collect();
        let too_many_slices: Vec<&[u8]> = too_many_strings.iter().map(|s| s.as_bytes()).collect();
        let too_many = ParamBufOwned::new(&too_many_slices);
        assert_eq!(too_many.len(), MAX_PARAMS);
        assert!(too_many.overflowed());

        // `ParamBuf::empty` and `ParamBufOwned::empty` agree.
        assert!(ParamBuf::empty().to_owned().is_empty());
        assert_eq!(ParamBuf::empty().to_owned(), ParamBufOwned::empty());
    }

    #[test]
    fn param_buf_owned_views_match_parser_views() {
        // A view over owned storage behaves exactly like a view over the
        // parser's inline storage holding the same parameters.
        let owned = ParamBufOwned::new(&[b"1", b"22", b""]);

        let mut list = ParamList::EMPTY;
        list.push_param(b"1");
        list.push_param(b"22");
        list.push_param(b"");
        let view = ParamBuf::from_param_list(&list);

        assert_eq!(owned.borrow(), view);
        assert_eq!(owned.borrow().len(), view.len());
        assert_eq!(owned.borrow().byte_len(), view.byte_len());
        assert_eq!(
            owned.numeric().try_write(&mut [0; 4]).unwrap(),
            view.numeric().try_write(&mut [0; 4]).unwrap()
        );
        assert_eq!(
            owned.borrow().into_iter().collect::<Vec<_>>(),
            view.into_iter().collect::<Vec<_>>()
        );
        assert_eq!(
            (&owned).into_iter().collect::<Vec<_>>(),
            view.into_iter().collect::<Vec<_>>()
        );
        assert_eq!(format!("{:?}", owned.borrow()), format!("{:?}", view));

        // `to_owned` of an owned-backed view is a deep copy.
        let copy = owned.borrow().to_owned();
        assert_eq!(copy, owned);
        assert_eq!(copy.get(1), Some(&b"22"[..]));

        // The owning iterator yields the same parameters as `Param`s.
        fn param(bytes: &[u8]) -> Param {
            let mut p = Param::EMPTY;
            p.extend_from_slice(bytes);
            p
        }
        assert_eq!(
            owned.into_iter().collect::<Vec<_>>(),
            vec![param(b"1"), param(b"22"), Param::EMPTY]
        );
    }

    #[test]
    fn param_buf_owned_overflowed_from_parser() {
        // Overflow survives the inline -> owned copy.
        let mut list = ParamList::EMPTY;
        list.push_param(&[b'7'; MAX_PARAM_LEN + 8]);
        list.push_param(b"1");
        let owned = ParamBuf::from_param_list(&list).to_owned();
        assert!(owned.overflowed());
        assert_eq!(owned.len(), 2);
        assert_eq!(owned.get(0).map(<[u8]>::len), Some(MAX_PARAM_LEN));

        // No overflow in the source, no overflow in the copy.
        let mut list = ParamList::EMPTY;
        list.push_param(b"1");
        let owned = ParamBuf::from_param_list(&list).to_owned();
        assert!(!owned.overflowed());
    }

    #[test]
    fn owned_param_size_is_pointer_sized_per_field() {
        // The owned buffer's inline footprint is two boxed slices (fat
        // pointers) plus a flag: a few words, far below the inline arena it
        // used to embed (lens + data + counts ~ 1.1 KiB).
        assert!(std::mem::size_of::<ParamBufOwned>() <= 5 * std::mem::size_of::<usize>());
        assert!(std::mem::size_of::<ParamList>() > 1024);
        assert!(std::mem::size_of::<ParamBufOwned>() * 16 < std::mem::size_of::<ParamList>());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn param_buf_owned_serde_shape_unchanged() {
        // The serialized shape is the same as before the `Box<[u8]>` change:
        // a map with a single "params" key holding the sequence.
        let owned = ParamBufOwned::new(&[b"1", b"22"]);
        let json = serde_json::to_string(&owned).unwrap();
        assert_eq!(json, r#"{"params":[[49],[50,50]]}"#);
    }
}
