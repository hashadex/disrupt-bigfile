//! This module provides fallible alternatives to [`Vec`]'s functions, because these functions are
//! either missing from the standard library or are not yet stable.
//!
//! In order to simplify error handling on the caller's side as much as possible, they take numbers
//! as an `impl TryInto<usize>` and merge [`TryFromIntError`] and [`TryReserveError`] into
//! [`Option::None`].

/// Tries to create a [`Vec`] with the specified `capacity`.
/// Stable alternative to [`Vec::try_with_capacity`].
pub fn try_with_capacity<T>(capacity: impl TryInto<usize>) -> Option<Vec<T>> {
    let mut vec = Vec::new();
    capacity
        .try_into()
        .ok()
        .and_then(|capacity| vec.try_reserve(capacity).ok())?;

    Some(vec)
}

/// Tries to create a [`Vec`] with some `amount` of default elements. Fallible alternative to
/// the `vec![T; N]` macro.
pub fn try_with_elements<T: Default + Clone>(amount: impl TryInto<usize>) -> Option<Vec<T>> {
    let amount = amount.try_into().ok()?;
    let value = T::default();

    let mut vec = try_with_capacity(amount)?;
    vec.resize(amount, value);

    Some(vec)
}
