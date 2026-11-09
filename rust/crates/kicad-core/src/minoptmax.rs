//@HEADER

//! Port of `libs/core/include/core/minoptmax.h`.

/// The numeric types `MINOPTMAX<T>` is instantiated with.
pub trait MinOptMaxValue: Copy + PartialOrd + Default {
    /// `std::numeric_limits<T>::max()`
    const MAX: Self;
}

impl MinOptMaxValue for i32 {
    const MAX: Self = i32::MAX;
}

impl MinOptMaxValue for i64 {
    const MAX: Self = i64::MAX;
}

impl MinOptMaxValue for f64 {
    const MAX: Self = f64::MAX;
}

/// Port of `template<class T=int> class MINOPTMAX`: an optional minimum, optimum and maximum
/// value (used by design rule constraints).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinOptMax<T: MinOptMaxValue = i32> {
    is_null: bool,
    min: T,
    opt: T,
    max: T,
    has_min: bool,
    has_opt: bool,
    has_max: bool,
}

impl<T: MinOptMaxValue> Default for MinOptMax<T> {
    fn default() -> Self {
        MinOptMax {
            is_null: true,
            min: T::default(),
            opt: T::default(),
            max: T::default(),
            has_min: false,
            has_opt: false,
            has_max: false,
        }
    }
}

/// `std::max<T>( a, b )`
fn std_max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { b } else { a }
}

/// `std::min<T>( a, b )`
fn std_min<T: PartialOrd>(a: T, b: T) -> T {
    if b < a { b } else { a }
}

impl<T: MinOptMaxValue> MinOptMax<T> {
    /// A null constraint value (the C++ default constructor).
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `MINOPTMAX::Min`: the minimum, or 0 if there is none.
    pub fn min(&self) -> T {
        if self.has_min { self.min } else { T::default() }
    }

    /// Port of `MINOPTMAX::Max`: the maximum, or the largest value of `T` if there is none.
    pub fn max(&self) -> T {
        if self.has_max { self.max } else { T::MAX }
    }

    /// Port of `MINOPTMAX::Opt`: the optimum, or `min()` if there is none.
    pub fn opt(&self) -> T {
        if self.has_opt { self.opt } else { self.min() }
    }

    /// Port of `MINOPTMAX::PinnedOpt()`: the optimum clamped to the min .. max range.
    pub fn pinned_opt(&self) -> T {
        std_max(self.min(), std_min(self.max(), self.opt()))
    }

    /// Port of `MINOPTMAX::PinnedOpt( T aDefault )`: like [`MinOptMax::pinned_opt`], but
    /// preferring `default` when there is no opt value.
    pub fn pinned_opt_or(&self, default: T) -> T {
        std_max(
            self.min(),
            std_min(self.max(), if self.has_opt { self.opt } else { default }),
        )
    }

    /// Port of `MINOPTMAX::HasMin`.
    pub fn has_min(&self) -> bool {
        self.has_min
    }

    /// Port of `MINOPTMAX::HasMax`.
    pub fn has_max(&self) -> bool {
        self.has_max
    }

    /// Port of `MINOPTMAX::HasOpt`.
    pub fn has_opt(&self) -> bool {
        self.has_opt
    }

    /// Port of `MINOPTMAX::SetMin`.
    pub fn set_min(&mut self, v: T) {
        self.is_null = false;
        self.min = v;
        self.has_min = true;
    }

    /// Port of `MINOPTMAX::SetMax`.
    pub fn set_max(&mut self, v: T) {
        self.is_null = false;
        self.max = v;
        self.has_max = true;
    }

    /// Port of `MINOPTMAX::SetOpt`.
    pub fn set_opt(&mut self, v: T) {
        self.is_null = false;
        self.opt = v;
        self.has_opt = true;
    }

    /// Port of `MINOPTMAX::IsNull`.
    pub fn is_null(&self) -> bool {
        self.is_null
    }

    /// Port of `MINOPTMAX::Matches`.
    pub fn matches(&self, v: T) -> bool {
        if self.has_min && v < self.min {
            return false;
        }

        if self.has_max && v > self.max {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_setters() {
        let mut v: MinOptMax = MinOptMax::new();

        assert!(v.is_null());
        assert_eq!(v.min(), 0);
        assert_eq!(v.max(), i32::MAX);
        assert_eq!(v.opt(), 0);
        assert!(v.matches(-5));

        v.set_min(10);
        assert!(!v.is_null());
        assert_eq!(v.opt(), 10);
        assert_eq!(v.pinned_opt_or(3), 10);
        assert!(!v.matches(9));

        v.set_max(20);
        v.set_opt(30);
        assert_eq!(v.opt(), 30);
        assert_eq!(v.pinned_opt(), 20);
        assert!(v.matches(20));
        assert!(!v.matches(21));
        assert!(v.has_min() && v.has_opt() && v.has_max());
    }
}
