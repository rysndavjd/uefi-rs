// SPDX-License-Identifier: MIT OR Apache-2.0

use uefi_raw::Status;

use super::UnalignedSlice;
use super::chars::{Char8, Char16, NUL_8, NUL_16};
use crate::mem::PoolAllocation;
use crate::polyfill::maybe_uninit_slice_assume_init_ref;
use core::borrow::Borrow;
use core::ffi::CStr;
use core::fmt::{self, Display, Formatter};
use core::mem::MaybeUninit;
use core::ops::Deref;
use core::ptr::NonNull;
use core::{ptr, slice};

#[cfg(feature = "alloc")]
use super::CString16;

/// Error converting from a slice (which can contain interior nuls) to a string
/// type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FromSliceUntilNulError {
    /// An invalid character was encountered before the end of the slice.
    InvalidChar(usize),

    /// The does not contain a nul character.
    NoNul,
}

impl Display for FromSliceUntilNulError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChar(usize) => write!(f, "invalid character at index {usize}"),
            Self::NoNul => write!(f, "no nul character"),
        }
    }
}

impl core::error::Error for FromSliceUntilNulError {}

/// Error converting from a slice (which cannot contain interior nuls) to a
/// string type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FromSliceWithNulError {
    /// An invalid character was encountered before the end of the slice
    InvalidChar(usize),

    /// A null character was encountered before the end of the slice
    InteriorNul(usize),

    /// The slice was not null-terminated
    NotNulTerminated,
}

impl Display for FromSliceWithNulError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChar(usize) => write!(f, "invalid character at index {usize}"),
            Self::InteriorNul(usize) => write!(f, "interior null character at index {usize}"),
            Self::NotNulTerminated => write!(f, "not null-terminated"),
        }
    }
}

impl core::error::Error for FromSliceWithNulError {}

/// Error returned by [`CStr16::from_unaligned_slice`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnalignedCStr16Error {
    /// An invalid character was encountered.
    InvalidChar(usize),

    /// A null character was encountered before the end of the data.
    InteriorNul(usize),

    /// The data was not null-terminated.
    NotNulTerminated,

    /// The buffer is not big enough to hold the entire string and
    /// trailing null character.
    BufferTooSmall,
}

impl Display for UnalignedCStr16Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChar(usize) => write!(f, "invalid character at index {usize}"),
            Self::InteriorNul(usize) => write!(f, "interior null character at index {usize}"),
            Self::NotNulTerminated => write!(f, "not null-terminated"),
            Self::BufferTooSmall => write!(f, "buffer too small"),
        }
    }
}

impl core::error::Error for UnalignedCStr16Error {}

/// Error returned by [`CStr16::from_str_with_buf`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FromStrWithBufError {
    /// An invalid character was encountered before the end of the string
    InvalidChar(usize),

    /// A null character was encountered in the string
    InteriorNul(usize),

    /// The buffer is not big enough to hold the entire string and
    /// trailing null character
    BufferTooSmall,
}

impl Display for FromStrWithBufError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChar(usize) => write!(f, "invalid character at index {usize}"),
            Self::InteriorNul(usize) => write!(f, "interior null character at index {usize}"),
            Self::BufferTooSmall => write!(f, "buffer too small"),
        }
    }
}

impl core::error::Error for FromStrWithBufError {}

/// A null-terminated Latin-1 string.
///
/// This type is largely inspired by [`core::ffi::CStr`] with the exception that all characters are
/// guaranteed to be 8 bit long.
///
/// A [`CStr8`] can be constructed from a [`core::ffi::CStr`] via a `try_from` call:
/// ```ignore
/// let cstr8: &CStr8 = TryFrom::try_from(cstr).unwrap();
/// ```
///
/// For convenience, a [`CStr8`] is comparable with [`core::str`] and
/// `alloc::string::String` from the standard library through the trait [`EqStrUntilNul`].
#[repr(transparent)]
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CStr8([Char8]);

impl CStr8 {
    /// Takes a raw pointer to a null-terminated Latin-1 string and wraps it in a CStr8 reference.
    ///
    /// # Safety
    ///
    /// The function will start accessing memory from `ptr` until the first
    /// null byte. It's the callers responsibility to ensure `ptr` points to
    /// a valid null-terminated string in accessible memory.
    #[must_use]
    pub unsafe fn from_ptr<'ptr>(ptr: *const Char8) -> &'ptr Self {
        let mut len = 0;
        while unsafe { *ptr.add(len) } != NUL_8 {
            len += 1
        }
        let ptr = ptr.cast::<u8>();
        unsafe { Self::from_bytes_with_nul_unchecked(slice::from_raw_parts(ptr, len + 1)) }
    }

    /// Creates a CStr8 reference from bytes.
    pub fn from_bytes_with_nul(chars: &[u8]) -> Result<&Self, FromSliceWithNulError> {
        let nul_pos = chars.iter().position(|&c| c == 0);
        if let Some(nul_pos) = nul_pos {
            if nul_pos + 1 != chars.len() {
                return Err(FromSliceWithNulError::InteriorNul(nul_pos));
            }
            Ok(unsafe { Self::from_bytes_with_nul_unchecked(chars) })
        } else {
            Err(FromSliceWithNulError::NotNulTerminated)
        }
    }

    /// Unsafely creates a CStr8 reference from bytes.
    ///
    /// # Safety
    ///
    /// It's the callers responsibility to ensure chars is a valid Latin-1
    /// null-terminated string, with no interior null bytes.
    #[must_use]
    pub const unsafe fn from_bytes_with_nul_unchecked(chars: &[u8]) -> &Self {
        unsafe { &*(ptr::from_ref(chars) as *const Self) }
    }

    /// Returns the inner pointer to this CStr8.
    #[must_use]
    pub const fn as_ptr(&self) -> *const Char8 {
        self.0.as_ptr()
    }

    /// Returns the underlying bytes as slice including the terminating null
    /// character.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        unsafe { &*(ptr::from_ref(&self.0) as *const [u8]) }
    }
}

impl fmt::Debug for CStr8 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CStr8({:?})", &self.0)
    }
}

impl fmt::Display for CStr8 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for c in &self.0[..&self.0.len() - 1] {
            <Char8 as fmt::Display>::fmt(c, f)?;
        }
        Ok(())
    }
}

impl AsRef<[u8]> for CStr8 {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Borrow<[u8]> for CStr8 {
    fn borrow(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl<StrType: AsRef<str> + ?Sized> EqStrUntilNul<StrType> for CStr8 {
    fn eq_str_until_nul(&self, other: &StrType) -> bool {
        let other = other.as_ref();

        // TODO: CStr16 has .iter() implemented, CStr8 not yet
        let any_not_equal = self
            .0
            .iter()
            .copied()
            .map(char::from)
            .zip(other.chars())
            // This only works as CStr8 is guaranteed to have a fixed character length
            // (unlike UTF-8).
            .take_while(|(l, r)| *l != '\0' && *r != '\0')
            .any(|(l, r)| l != r);

        !any_not_equal
    }
}

impl<'a> TryFrom<&'a CStr> for &'a CStr8 {
    type Error = FromSliceWithNulError;

    fn try_from(cstr: &'a CStr) -> Result<Self, Self::Error> {
        CStr8::from_bytes_with_nul(cstr.to_bytes_with_nul())
    }
}

/// Get a Latin-1 character from a UTF-8 byte slice at the given offset.
///
/// Returns a pair containing the Latin-1 character and the number of bytes in
/// the UTF-8 encoding of that character.
///
/// Panics if the string cannot be encoded in Latin-1.
///
/// # Safety
///
/// The input `bytes` must be valid UTF-8.
const unsafe fn latin1_from_utf8_at_offset(bytes: &[u8], offset: usize) -> (u8, usize) {
    if bytes[offset] & 0b1000_0000 == 0b0000_0000 {
        (bytes[offset], 1)
    } else if bytes[offset] & 0b1110_0000 == 0b1100_0000 {
        let a = (bytes[offset] & 0b0001_1111) as u16;
        let b = (bytes[offset + 1] & 0b0011_1111) as u16;
        let ch = (a << 6) | b;
        if ch > 0xff {
            panic!("input string cannot be encoded as Latin-1");
        }
        (ch as u8, 2)
    } else {
        // Latin-1 code points only go up to 0xff, so if the input contains any
        // UTF-8 characters larger than two bytes it cannot be converted to
        // Latin-1.
        panic!("input string cannot be encoded as Latin-1");
    }
}

/// Count the number of Latin-1 characters in a string.
///
/// Panics if the string cannot be encoded in Latin-1.
///
/// This is public but hidden; it is used in the `cstr8` macro.
#[must_use]
pub const fn str_num_latin1_chars(s: &str) -> usize {
    let bytes = s.as_bytes();
    let len = bytes.len();

    let mut offset = 0;
    let mut num_latin1_chars = 0;

    while offset < len {
        // SAFETY: `bytes` is valid UTF-8.
        let (_, num_utf8_bytes) = unsafe { latin1_from_utf8_at_offset(bytes, offset) };
        offset += num_utf8_bytes;
        num_latin1_chars += 1;
    }

    num_latin1_chars
}

/// Convert a `str` into a null-terminated Latin-1 character array.
///
/// Panics if the string cannot be encoded in Latin-1.
///
/// This is public but hidden; it is used in the `cstr8` macro.
#[must_use]
pub const fn str_to_latin1<const N: usize>(s: &str) -> [u8; N] {
    let bytes = s.as_bytes();
    let len = bytes.len();

    let mut output = [0; N];

    let mut output_offset = 0;
    let mut input_offset = 0;
    while input_offset < len {
        // SAFETY: `bytes` is valid UTF-8.
        let (ch, num_utf8_bytes) = unsafe { latin1_from_utf8_at_offset(bytes, input_offset) };
        if ch == 0 {
            panic!("interior null character");
        } else {
            output[output_offset] = ch;
            output_offset += 1;
            input_offset += num_utf8_bytes;
        }
    }

    // The output array must be one bigger than the converted string,
    // to leave room for the trailing null character.
    if output_offset + 1 != N {
        panic!("incorrect array length");
    }

    output
}

/// An UCS-2 null-terminated string slice.
///
/// This type is largely inspired by [`core::ffi::CStr`] with the exception that all characters are
/// guaranteed to be 16 bit long.
///
/// For convenience, a [`CStr16`] is comparable with [`core::str`] and
/// `alloc::string::String` from the standard library through the trait [`EqStrUntilNul`].
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(transparent)]
pub struct CStr16([Char16]);

impl CStr16 {
    /// Wraps a raw UEFI string with a safe C string wrapper
    ///
    /// # Safety
    ///
    /// The function will start accessing memory from `ptr` until the first
    /// null character. It's the callers responsibility to ensure `ptr` points to
    /// a valid string, in accessible memory.
    #[must_use]
    pub unsafe fn from_ptr<'ptr>(ptr: *const Char16) -> &'ptr Self {
        let mut len = 0;
        while unsafe { *ptr.add(len) } != NUL_16 {
            len += 1
        }
        let ptr = ptr.cast::<u16>();
        unsafe { Self::from_u16_with_nul_unchecked(slice::from_raw_parts(ptr, len + 1)) }
    }

    /// Creates a `&CStr16` from a u16 slice, stopping at the first nul character.
    ///
    /// # Errors
    ///
    /// An error is returned if the slice contains invalid UCS-2 characters, or
    /// if the slice does not contain any nul character.
    pub fn from_u16_until_nul(codes: &[u16]) -> Result<&Self, FromSliceUntilNulError> {
        for (pos, &code) in codes.iter().enumerate() {
            let chr =
                Char16::try_from(code).map_err(|_| FromSliceUntilNulError::InvalidChar(pos))?;
            if chr == NUL_16 {
                return Ok(unsafe { Self::from_u16_with_nul_unchecked(&codes[..=pos]) });
            }
        }
        Err(FromSliceUntilNulError::NoNul)
    }

    /// Creates a `&CStr16` from a u16 slice, if the slice contains exactly
    /// one terminating null-byte and all chars are valid UCS-2 chars.
    pub fn from_u16_with_nul(codes: &[u16]) -> Result<&Self, FromSliceWithNulError> {
        for (pos, &code) in codes.iter().enumerate() {
            match code.try_into() {
                Ok(NUL_16) => {
                    if pos != codes.len() - 1 {
                        return Err(FromSliceWithNulError::InteriorNul(pos));
                    } else {
                        return Ok(unsafe { Self::from_u16_with_nul_unchecked(codes) });
                    }
                }
                Err(_) => {
                    return Err(FromSliceWithNulError::InvalidChar(pos));
                }
                _ => {}
            }
        }
        Err(FromSliceWithNulError::NotNulTerminated)
    }

    /// Unsafely creates a `&CStr16` from a u16 slice.
    ///
    /// # Safety
    ///
    /// It's the callers responsibility to ensure chars is a valid UCS-2
    /// null-terminated string, with no interior null characters.
    #[must_use]
    pub const unsafe fn from_u16_with_nul_unchecked(codes: &[u16]) -> &Self {
        unsafe { &*(ptr::from_ref(codes) as *const Self) }
    }

    /// Creates a `&CStr16` from a [`Char16`] slice, stopping at the first nul character.
    ///
    /// # Errors
    ///
    /// An error is returned if the slice does not contain any nul character.
    pub fn from_char16_until_nul(chars: &[Char16]) -> Result<&Self, FromSliceUntilNulError> {
        // Find the index of the first null char.
        let end = chars
            .iter()
            .position(|c| *c == NUL_16)
            .ok_or(FromSliceUntilNulError::NoNul)?;

        // Safety: the input is nul-terminated.
        unsafe { Ok(Self::from_char16_with_nul_unchecked(&chars[..=end])) }
    }

    /// Creates a `&CStr16` from a [`Char16`] slice, if the slice is
    /// null-terminated and has no interior null characters.
    pub fn from_char16_with_nul(chars: &[Char16]) -> Result<&Self, FromSliceWithNulError> {
        // Fail early if the input is empty.
        if chars.is_empty() {
            return Err(FromSliceWithNulError::NotNulTerminated);
        }

        // Find the index of the first null char.
        if let Some(null_index) = chars.iter().position(|c| *c == NUL_16) {
            // Verify the null character is at the end.
            if null_index == chars.len() - 1 {
                // Safety: the input is null-terminated and has no interior nulls.
                Ok(unsafe { Self::from_char16_with_nul_unchecked(chars) })
            } else {
                Err(FromSliceWithNulError::InteriorNul(null_index))
            }
        } else {
            Err(FromSliceWithNulError::NotNulTerminated)
        }
    }

    /// Unsafely creates a `&CStr16` from a `Char16` slice.
    ///
    /// # Safety
    ///
    /// It's the callers responsibility to ensure chars is null-terminated and
    /// has no interior null characters.
    #[must_use]
    pub const unsafe fn from_char16_with_nul_unchecked(chars: &[Char16]) -> &Self {
        let ptr: *const [Char16] = chars;
        unsafe { &*(ptr as *const Self) }
    }

    /// Convert a [`&str`] to a `&CStr16`, backed by a buffer.
    ///
    /// The input string must contain only characters representable with
    /// UCS-2, and must not contain any null characters (even at the end of
    /// the input).
    ///
    /// The backing buffer must be big enough to hold the converted string as
    /// well as a trailing null character.
    ///
    /// # Examples
    ///
    /// Convert the UTF-8 string "ABC" to a `&CStr16`:
    ///
    /// ```
    /// use uefi::CStr16;
    ///
    /// let mut buf = [0; 4];
    /// CStr16::from_str_with_buf("ABC", &mut buf).unwrap();
    /// ```
    pub fn from_str_with_buf<'a>(
        input: &str,
        buf: &'a mut [u16],
    ) -> Result<&'a Self, FromStrWithBufError> {
        let mut index = 0;

        // Convert to UTF-16.
        for c in input.encode_utf16() {
            *buf.get_mut(index)
                .ok_or(FromStrWithBufError::BufferTooSmall)? = c;
            index += 1;
        }

        // Add trailing null character.
        *buf.get_mut(index)
            .ok_or(FromStrWithBufError::BufferTooSmall)? = 0;

        // Convert from u16 to Char16. This checks for invalid UCS-2 chars and
        // interior nulls. The NotNulTerminated case is unreachable because we
        // just added a trailing null character.
        Self::from_u16_with_nul(&buf[..index + 1]).map_err(|err| match err {
            FromSliceWithNulError::InvalidChar(p) => FromStrWithBufError::InvalidChar(p),
            FromSliceWithNulError::InteriorNul(p) => FromStrWithBufError::InteriorNul(p),
            FromSliceWithNulError::NotNulTerminated => {
                unreachable!()
            }
        })
    }

    /// Create a `&CStr16` from an [`UnalignedSlice`] using an aligned
    /// buffer for storage. The lifetime of the output is tied to `buf`,
    /// not `src`.
    pub fn from_unaligned_slice<'buf>(
        src: &UnalignedSlice<'_, u16>,
        buf: &'buf mut [MaybeUninit<u16>],
    ) -> Result<&'buf Self, UnalignedCStr16Error> {
        // The input `buf` might be longer than needed, so get a
        // subslice of the required length.
        let buf = buf
            .get_mut(..src.len())
            .ok_or(UnalignedCStr16Error::BufferTooSmall)?;

        src.copy_to_maybe_uninit(buf);
        let buf = unsafe {
            // Safety: `copy_buf` fully initializes the slice.
            maybe_uninit_slice_assume_init_ref(buf)
        };
        Self::from_u16_with_nul(buf).map_err(|e| match e {
            FromSliceWithNulError::InvalidChar(v) => UnalignedCStr16Error::InvalidChar(v),
            FromSliceWithNulError::InteriorNul(v) => UnalignedCStr16Error::InteriorNul(v),
            FromSliceWithNulError::NotNulTerminated => UnalignedCStr16Error::NotNulTerminated,
        })
    }

    /// Returns the inner pointer to this C16 string.
    #[must_use]
    pub const fn as_ptr(&self) -> *const Char16 {
        self.0.as_ptr()
    }

    /// Get the underlying [`Char16`]s as slice without the trailing null.
    #[must_use]
    pub fn as_slice(&self) -> &[Char16] {
        &self.0[..self.num_chars()]
    }

    /// Get the underlying [`Char16`]s as slice including the trailing null.
    #[must_use]
    pub const fn as_slice_with_nul(&self) -> &[Char16] {
        &self.0
    }

    /// Converts this C string to a u16 slice without the trailing null.
    #[must_use]
    pub fn to_u16_slice(&self) -> &[u16] {
        let chars = self.to_u16_slice_with_nul();
        &chars[..chars.len() - 1]
    }

    /// Converts this C string to a u16 slice containing the trailing null.
    #[must_use]
    pub const fn to_u16_slice_with_nul(&self) -> &[u16] {
        unsafe { &*(ptr::from_ref(&self.0) as *const [u16]) }
    }

    /// Returns an iterator over this C string
    #[must_use]
    pub const fn iter(&self) -> CStr16Iter<'_> {
        CStr16Iter {
            inner: self,
            pos: 0,
        }
    }

    /// Returns the number of characters without the trailing null. character
    #[must_use]
    pub const fn num_chars(&self) -> usize {
        self.0.len() - 1
    }

    /// Returns if the string is empty. This ignores the null character.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.num_chars() == 0
    }

    /// Get the number of bytes in the string (including the trailing null).
    #[must_use]
    pub const fn num_bytes(&self) -> usize {
        self.0.len() * 2
    }

    /// Checks if all characters in this string are within the ASCII range.
    #[must_use]
    pub fn is_ascii(&self) -> bool {
        self.0.iter().all(|c| c.is_ascii())
    }

    /// Writes each [`Char16`] as a [`char`] (4 bytes long in Rust language) into the buffer.
    /// It is up to the implementer of [`core::fmt::Write`] to convert the char to a string
    /// with proper encoding/charset. For example, in the case of [`alloc::string::String`]
    /// all Rust chars (UTF-32) get converted to UTF-8.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// let firmware_vendor_c16_str: CStr16 = ...;
    /// // crate "arrayvec" uses stack-allocated arrays for Strings => no heap allocations
    /// let mut buf = arrayvec::ArrayString::<128>::new();
    /// firmware_vendor_c16_str.as_str_in_buf(&mut buf);
    /// log::info!("as rust str: {}", buf.as_str());
    /// ```
    ///
    /// [`alloc::string::String`]: https://doc.rust-lang.org/nightly/alloc/string/struct.String.html
    pub fn as_str_in_buf(&self, buf: &mut dyn core::fmt::Write) -> core::fmt::Result {
        for c16 in self.iter() {
            buf.write_char(char::from(*c16))?;
        }
        Ok(())
    }

    /// Returns the underlying bytes as slice including the terminating null
    /// character.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.0.as_ptr().cast(), self.num_bytes()) }
    }
}

impl AsRef<[u8]> for CStr16 {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Borrow<[u8]> for CStr16 {
    fn borrow(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(feature = "alloc")]
impl From<&CStr16> for alloc::string::String {
    fn from(value: &CStr16) -> Self {
        value
            .as_slice()
            .iter()
            .copied()
            .map(u16::from)
            .map(u32::from)
            .map(|int| char::from_u32(int).expect("Should be encodable as UTF-8"))
            .collect::<Self>()
    }
}

impl<StrType: AsRef<str> + ?Sized> EqStrUntilNul<StrType> for CStr16 {
    fn eq_str_until_nul(&self, other: &StrType) -> bool {
        let other = other.as_ref();

        let any_not_equal = self
            .iter()
            .copied()
            .map(char::from)
            .zip(other.chars())
            // This only works as CStr16 is guaranteed to have a fixed character length
            // (unlike UTF-8 or UTF-16).
            .take_while(|(l, r)| *l != '\0' && *r != '\0')
            .any(|(l, r)| l != r);

        !any_not_equal
    }
}

impl AsRef<Self> for CStr16 {
    fn as_ref(&self) -> &Self {
        self
    }
}

/// An iterator over the [`Char16`]s in a [`CStr16`].
#[derive(Debug)]
pub struct CStr16Iter<'a> {
    inner: &'a CStr16,
    pos: usize,
}

impl<'a> Iterator for CStr16Iter<'a> {
    type Item = &'a Char16;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.inner.0.len() - 1 {
            None
        } else {
            self.pos += 1;
            self.inner.0.get(self.pos - 1)
        }
    }
}

impl fmt::Debug for CStr16 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CStr16({:?})", &self.0)
    }
}

impl fmt::Display for CStr16 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for c in self.iter() {
            <Char16 as fmt::Display>::fmt(c, f)?;
        }
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl PartialEq<CString16> for &CStr16 {
    fn eq(&self, other: &CString16) -> bool {
        PartialEq::eq(*self, other.as_ref())
    }
}

/// UCS-2 string allocated from UEFI pool memory.
///
/// This is similar to a [`CString16`], but used for memory that was allocated
/// internally by UEFI rather than the Rust allocator.
///
/// [`CString16`]: crate::CString16
#[derive(Debug)]
pub struct PoolString(PoolAllocation);

impl PoolString {
    /// Create a [`PoolString`] from a [`CStr16`] residing in a buffer allocated
    /// using [`allocate_pool()`][cbap].
    ///
    /// # Safety
    ///
    /// The caller must ensure that the buffer points to a valid [`CStr16`] and
    /// resides in a buffer allocated using [`allocate_pool()`][cbap]
    ///
    /// [cbap]: crate::boot::allocate_pool()
    pub unsafe fn new(text: *const Char16) -> crate::Result<Self> {
        NonNull::new(text.cast_mut())
            .map(|p| Self(PoolAllocation::new(p.cast())))
            .ok_or(Status::OUT_OF_RESOURCES.into())
    }
}

impl Deref for PoolString {
    type Target = CStr16;

    fn deref(&self) -> &Self::Target {
        unsafe { CStr16::from_ptr(self.0.as_ptr().as_ptr().cast()) }
    }
}

impl UnalignedSlice<'_, u16> {
    /// Create a [`CStr16`] from an [`UnalignedSlice`] using an aligned
    /// buffer for storage. The lifetime of the output is tied to `buf`,
    /// not `self`.
    pub fn to_cstr16<'buf>(
        &self,
        buf: &'buf mut [MaybeUninit<u16>],
    ) -> Result<&'buf CStr16, UnalignedCStr16Error> {
        CStr16::from_unaligned_slice(self, buf)
    }
}

/// The EqStrUntilNul trait helps to compare Rust strings against UEFI string types (UCS-2 strings).
/// The given generic implementation of this trait enables us that we only have to
/// implement one direction (`left.eq_str_until_nul(&right)`) for each UEFI string type and we
/// get the other direction (`right.eq_str_until_nul(&left)`) for free. Hence, the relation is
/// reflexive.
pub trait EqStrUntilNul<StrType: ?Sized> {
    /// Checks if the provided Rust string `StrType` is equal to [Self] until the first null character
    /// is found. An exception is the terminating null character of [Self] which is ignored.
    ///
    /// As soon as the first null character in either `&self` or `other` is found, this method returns.
    /// Note that Rust strings are allowed to contain null bytes that do not terminate the string.
    /// Although this is rather unusual, you can compare `"foo\0bar"` with an instance of [Self].
    /// In that case, only `foo"` is compared against [Self] (if [Self] is long enough).
    fn eq_str_until_nul(&self, other: &StrType) -> bool;
}

// magic implementation which transforms an existing `left.eq_str_until_nul(&right)` implementation
// into an additional working `right.eq_str_until_nul(&left)` implementation.
impl<StrType, C16StrType> EqStrUntilNul<C16StrType> for StrType
where
    StrType: AsRef<str>,
    C16StrType: EqStrUntilNul<StrType> + ?Sized,
{
    fn eq_str_until_nul(&self, other: &C16StrType) -> bool {
        // reuse the existing implementation
        other.eq_str_until_nul(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cstr8, cstr16};
    use alloc::format;
    use alloc::string::String;

    // Tests if our CStr8 type can be constructed from a valid core::ffi::CStr
    #[test]
    fn test_cstr8_from_cstr() {
        let msg = "hello world\0";
        let cstr = unsafe { CStr::from_ptr(msg.as_ptr().cast()) };
        let cstr8: &CStr8 = TryFrom::try_from(cstr).unwrap();
        assert!(cstr8.eq_str_until_nul(msg));
        assert!(msg.eq_str_until_nul(cstr8));
    }

    #[test]
    fn test_cstr8_as_bytes() {
        let string: &CStr8 = cstr8!("a");
        assert_eq!(string.as_bytes(), &[b'a', 0]);
        assert_eq!(<CStr8 as AsRef<[u8]>>::as_ref(string), &[b'a', 0]);
        assert_eq!(<CStr8 as Borrow<[u8]>>::borrow(string), &[b'a', 0]);
    }

    #[test]
    fn test_cstr8_display() {
        let s = cstr8!("abc");
        assert_eq!(format!("{s}"), "abc");
    }

    #[test]
    fn test_cstr16_display() {
        let s = cstr16!("abc");
        assert_eq!(format!("{s}"), "abc");
    }

    #[test]
    fn test_cstr16_num_bytes() {
        let s = CStr16::from_u16_with_nul(&[65, 66, 67, 0]).unwrap();
        assert_eq!(s.num_bytes(), 8);
    }

    #[test]
    fn test_cstr16_from_u16_until_nul() {
        // Invalid: empty input.
        assert_eq!(
            CStr16::from_u16_until_nul(&[]),
            Err(FromSliceUntilNulError::NoNul)
        );

        // Invalid: no nul.
        assert_eq!(
            CStr16::from_u16_until_nul(&[65, 66]),
            Err(FromSliceUntilNulError::NoNul)
        );

        // Invalid: not UCS-2.
        assert_eq!(
            CStr16::from_u16_until_nul(&[65, 0xde01, 0]),
            Err(FromSliceUntilNulError::InvalidChar(1))
        );

        // Valid: trailing nul.
        assert_eq!(CStr16::from_u16_until_nul(&[97, 98, 0,]), Ok(cstr16!("ab")));

        // Valid: interior nul.
        assert_eq!(
            CStr16::from_u16_until_nul(&[97, 0, 98, 0,]),
            Ok(cstr16!("a"))
        );
    }

    #[test]
    fn test_cstr16_from_char16_until_nul() {
        // Invalid: empty input.
        assert_eq!(
            CStr16::from_char16_until_nul(&[]),
            Err(FromSliceUntilNulError::NoNul)
        );

        // Invalid: no nul character.
        assert_eq!(
            CStr16::from_char16_until_nul(&[
                Char16::try_from('a').unwrap(),
                Char16::try_from('b').unwrap(),
            ]),
            Err(FromSliceUntilNulError::NoNul)
        );

        // Valid: trailing nul.
        assert_eq!(
            CStr16::from_char16_until_nul(&[
                Char16::try_from('a').unwrap(),
                Char16::try_from('b').unwrap(),
                NUL_16,
            ]),
            Ok(cstr16!("ab"))
        );

        // Valid: interior nul.
        assert_eq!(
            CStr16::from_char16_until_nul(&[
                Char16::try_from('a').unwrap(),
                NUL_16,
                Char16::try_from('b').unwrap(),
                NUL_16
            ]),
            Ok(cstr16!("a"))
        );
    }

    #[test]
    fn test_cstr16_from_char16_with_nul() {
        // Invalid: empty input.
        assert_eq!(
            CStr16::from_char16_with_nul(&[]),
            Err(FromSliceWithNulError::NotNulTerminated)
        );

        // Invalid: interior null.
        assert_eq!(
            CStr16::from_char16_with_nul(&[
                Char16::try_from('a').unwrap(),
                NUL_16,
                Char16::try_from('b').unwrap(),
                NUL_16
            ]),
            Err(FromSliceWithNulError::InteriorNul(1))
        );

        // Invalid: no trailing null.
        assert_eq!(
            CStr16::from_char16_with_nul(&[
                Char16::try_from('a').unwrap(),
                Char16::try_from('b').unwrap(),
            ]),
            Err(FromSliceWithNulError::NotNulTerminated)
        );

        // Valid.
        assert_eq!(
            CStr16::from_char16_with_nul(&[
                Char16::try_from('a').unwrap(),
                Char16::try_from('b').unwrap(),
                NUL_16,
            ]),
            Ok(cstr16!("ab"))
        );
    }

    #[test]
    fn test_cstr16_from_str_with_buf() {
        let mut buf = [0; 4];

        // OK: buf is exactly the right size.
        let s = CStr16::from_str_with_buf("ABC", &mut buf).unwrap();
        assert_eq!(s.to_u16_slice_with_nul(), [65, 66, 67, 0]);

        // OK: buf is bigger than needed.
        let s = CStr16::from_str_with_buf("A", &mut buf).unwrap();
        assert_eq!(s.to_u16_slice_with_nul(), [65, 0]);

        // Error: buf is too small.
        assert_eq!(
            CStr16::from_str_with_buf("ABCD", &mut buf).unwrap_err(),
            FromStrWithBufError::BufferTooSmall
        );

        // Error: invalid character.
        assert_eq!(
            CStr16::from_str_with_buf("a😀", &mut buf).unwrap_err(),
            FromStrWithBufError::InvalidChar(1),
        );

        // Error: interior null.
        assert_eq!(
            CStr16::from_str_with_buf("a\0b", &mut buf).unwrap_err(),
            FromStrWithBufError::InteriorNul(1),
        );
    }

    #[test]
    fn test_cstr16_macro() {
        // Just a sanity check to make sure it's spitting out the right characters
        assert_eq!(
            crate::prelude::cstr16!("ABC").to_u16_slice_with_nul(),
            [65, 66, 67, 0]
        )
    }

    #[test]
    fn test_unaligned_cstr16() {
        let mut buf = [0u16; 6];
        let us = unsafe {
            let ptr = buf.as_mut_ptr().cast::<u8>();
            // Intentionally create an unaligned u16 pointer. This
            // leaves room for five u16 characters.
            let ptr = ptr.add(1).cast::<u16>();
            // Write out the "test" string.
            ptr.add(0).write_unaligned(b't'.into());
            ptr.add(1).write_unaligned(b'e'.into());
            ptr.add(2).write_unaligned(b's'.into());
            ptr.add(3).write_unaligned(b't'.into());
            ptr.add(4).write_unaligned(b'\0'.into());

            // Create the `UnalignedSlice`.
            UnalignedSlice::new(ptr, 5)
        };

        // Test `to_cstr16()` with too small of a buffer.
        let mut buf = [MaybeUninit::new(0); 4];
        assert_eq!(
            us.to_cstr16(&mut buf).unwrap_err(),
            UnalignedCStr16Error::BufferTooSmall
        );
        // Test with a big enough buffer.
        let mut buf = [MaybeUninit::new(0); 5];
        assert_eq!(
            us.to_cstr16(&mut buf).unwrap(),
            CString16::try_from("test").unwrap()
        );

        // Test `to_cstring16()`.
        assert_eq!(
            us.to_cstring16().unwrap(),
            CString16::try_from("test").unwrap()
        );
    }

    #[test]
    fn test_cstr16_as_slice() {
        let string: &CStr16 = cstr16!("a");
        assert_eq!(string.as_slice(), &[Char16::try_from('a').unwrap()]);
        assert_eq!(
            string.as_slice_with_nul(),
            &[Char16::try_from('a').unwrap(), NUL_16]
        );
    }

    #[test]
    fn test_cstr16_as_bytes() {
        let string: &CStr16 = cstr16!("a");
        assert_eq!(string.as_bytes(), &[b'a', 0, 0, 0]);
        assert_eq!(<CStr16 as AsRef<[u8]>>::as_ref(string), &[b'a', 0, 0, 0]);
        assert_eq!(<CStr16 as Borrow<[u8]>>::borrow(string), &[b'a', 0, 0, 0]);
    }

    // Code generation helper for the compare tests of our CStrX types against "str" and "String"
    // from the standard library.
    #[allow(non_snake_case)]
    macro_rules! test_compare_cstrX {
        ($input:ident) => {
            assert!($input.eq_str_until_nul(&"test"));
            assert!($input.eq_str_until_nul(&String::from("test")));

            // now other direction
            assert!(String::from("test").eq_str_until_nul($input));
            assert!("test".eq_str_until_nul($input));

            // some more tests
            // this is fine: compare until the first null
            assert!($input.eq_str_until_nul(&"te\0st"));
            // this is fine
            assert!($input.eq_str_until_nul(&"test\0"));
            assert!(!$input.eq_str_until_nul(&"hello"));
        };
    }

    #[test]
    fn test_compare_cstr8() {
        // test various comparisons with different order (left, right)
        let input: &CStr8 = cstr8!("test");
        test_compare_cstrX!(input);
    }

    #[test]
    fn test_compare_cstr16() {
        let input: &CStr16 = cstr16!("test");
        test_compare_cstrX!(input);
    }

    /// Test that the `cstr16!` macro can be used in a `const` context.
    #[test]
    fn test_cstr16_macro_const() {
        const S: &CStr16 = cstr16!("ABC");
        assert_eq!(S.to_u16_slice_with_nul(), [65, 66, 67, 0]);
    }

    /// Tests the trait implementation of trait [`EqStrUntilNul]` for [`CStr8`].
    ///
    /// This tests that `String` and `str` from the standard library can be
    /// checked for equality against a [`CStr8`]. It checks both directions,
    /// i.e., the equality is reflexive.
    #[test]
    fn test_cstr8_eq_std_str() {
        let input: &CStr8 = cstr8!("test");

        // test various comparisons with different order (left, right)
        assert!(input.eq_str_until_nul("test")); // requires ?Sized constraint
        assert!(input.eq_str_until_nul(&"test"));
        assert!(input.eq_str_until_nul(&String::from("test")));

        // now other direction
        assert!(String::from("test").eq_str_until_nul(input));
        assert!("test".eq_str_until_nul(input));
    }

    /// Tests the trait implementation of trait [`EqStrUntilNul]` for [`CStr16`].
    ///
    /// This tests that `String` and `str` from the standard library can be
    /// checked for equality against a [`CStr16`]. It checks both directions,
    /// i.e., the equality is reflexive.
    #[test]
    fn test_cstr16_eq_std_str() {
        let input: &CStr16 = cstr16!("test");

        assert!(input.eq_str_until_nul("test")); // requires ?Sized constraint
        assert!(input.eq_str_until_nul(&"test"));
        assert!(input.eq_str_until_nul(&String::from("test")));

        // now other direction
        assert!(String::from("test").eq_str_until_nul(input));
        assert!("test".eq_str_until_nul(input));
    }
}
