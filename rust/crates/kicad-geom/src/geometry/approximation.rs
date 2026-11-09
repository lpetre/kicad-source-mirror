//LICENSE

//! Port of `libs/kimath/include/geometry/approximation.h`.

/// Port of `ERROR_LOC`.
///
/// When approximating an arc or circle, should the error be placed on the outside
/// or inside of the curve?  (Generally speaking filled shape errors go on the inside
/// and knockout errors go on the outside.  This preserves minimum clearances.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum ErrorLoc {
    /// `ERROR_OUTSIDE`
    Outside = 0,
    /// `ERROR_INSIDE`
    Inside,
}

/// `ERROR_OUTSIDE`
pub const ERROR_OUTSIDE: ErrorLoc = ErrorLoc::Outside;
/// `ERROR_INSIDE`
pub const ERROR_INSIDE: ErrorLoc = ErrorLoc::Inside;
