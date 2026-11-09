//! `ClipperD`: the clipping engine on floating-point coordinates.

use crate::core::{ClipType, ClipperError, FillRule, MAX_DECIMAL_PRECISION, PathsD, scale_paths};
use crate::engine::{ClipperBase, PathType};
use crate::polytree::{PolyTree64, PolyTreeD};

/// Closed and open paths produced by [`ClipperD::execute`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolutionD {
    /// Closed paths (polygons).
    pub closed: PathsD,
    /// Open paths (polylines) from open subjects.
    pub open: PathsD,
}

/// Floating-point polygon clipper. Coordinates are scaled by a power of two of at least
/// `10^precision` into the integer engine and back.
pub struct ClipperD {
    base: ClipperBase,
    scale: f64,
    inv_scale: f64,
    #[cfg(feature = "usingz")]
    z_callback: Option<std::rc::Rc<std::cell::RefCell<ZCallbackD>>>,
}

/// Z callback for [`ClipperD`]: `(e1bot, e1top, e2bot, e2top, intersection_point)`.
#[cfg(feature = "usingz")]
pub type ZCallbackD =
    Box<dyn FnMut(crate::PointD, crate::PointD, crate::PointD, crate::PointD, &mut crate::PointD)>;

impl Default for ClipperD {
    /// Precision 2.
    fn default() -> Self {
        Self::new(2).expect("2 is a valid precision")
    }
}

impl ClipperD {
    /// `precision` is the number of decimal places kept, `-8..=8`.
    pub fn new(precision: i32) -> Result<Self, ClipperError> {
        if !(-MAX_DECIMAL_PRECISION..=MAX_DECIMAL_PRECISION).contains(&precision) {
            return Err(ClipperError::PrecisionOutOfRange);
        }
        // radix ^ (ilogb(10^precision) + 1): the power of two just above 10^precision.
        let p10 = 10f64.powi(precision);
        let scale = 2f64.powi(ilogb(p10) + 1);
        Ok(ClipperD {
            base: ClipperBase::default(),
            scale,
            inv_scale: 1.0 / scale,
            #[cfg(feature = "usingz")]
            z_callback: None,
        })
    }

    /// Keep collinear vertices in closed paths (default `true`).
    pub fn set_preserve_collinear(&mut self, value: bool) {
        self.base.preserve_collinear = value;
    }

    /// Whether collinear vertices are kept; see [`Self::set_preserve_collinear`].
    pub fn preserve_collinear(&self) -> bool {
        self.base.preserve_collinear
    }

    /// Reverse the orientation of every solution path (default `false`).
    pub fn set_reverse_solution(&mut self, value: bool) {
        self.base.reverse_solution = value;
    }

    /// Whether solution paths are reversed; see [`Self::set_reverse_solution`].
    pub fn reverse_solution(&self) -> bool {
        self.base.reverse_solution
    }

    fn add(
        &mut self,
        paths: &PathsD,
        polytype: PathType,
        is_open: bool,
    ) -> Result<(), ClipperError> {
        let scaled = scale_paths(paths, self.scale, self.scale)?;
        self.base.add_paths(&scaled, polytype, is_open);
        Ok(())
    }

    /// Adds closed subject paths; fails if a scaled coordinate is out of range.
    pub fn add_subject(&mut self, subjects: &PathsD) -> Result<(), ClipperError> {
        self.add(subjects, PathType::Subject, false)
    }

    /// Adds open subject paths (polylines); fails if a scaled coordinate is out of range.
    pub fn add_open_subject(&mut self, open_subjects: &PathsD) -> Result<(), ClipperError> {
        self.add(open_subjects, PathType::Subject, true)
    }

    /// Adds closed clip paths; fails if a scaled coordinate is out of range.
    pub fn add_clip(&mut self, clips: &PathsD) -> Result<(), ClipperError> {
        self.add(clips, PathType::Clip, false)
    }

    /// Removes every added path.
    pub fn clear(&mut self) {
        self.base.clear();
    }

    /// Sets the callback that assigns `z` to new intersection points.
    #[cfg(feature = "usingz")]
    pub fn set_z_callback(&mut self, cb: Option<ZCallbackD>) {
        self.z_callback = cb.map(|cb| std::rc::Rc::new(std::cell::RefCell::new(cb)));
    }

    /// Performs the boolean operation.
    pub fn execute(
        &mut self,
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Result<SolutionD, ClipperError> {
        self.install_z_callback();
        let inv_scale = self.inv_scale;
        let result = self.base.run(clip_type, fill_rule, false).map(|_| {
            let mut s = SolutionD::default();
            self.base
                .build_paths_d(&mut s.closed, &mut s.open, inv_scale);
            s
        });
        self.base.clean_up();
        self.restore_z_callback();
        result
    }

    /// Performs the boolean operation into a [`PolyTreeD`]; the second value holds the open paths.
    pub fn execute_tree(
        &mut self,
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Result<(PolyTreeD, PathsD), ClipperError> {
        self.install_z_callback();
        let inv_scale = self.inv_scale;
        let result = self.base.run(clip_type, fill_rule, true).map(|_| {
            let mut tree = PolyTree64::default();
            let mut open = Vec::new();
            self.base.build_tree_d(&mut tree, &mut open, inv_scale);
            // PolyPathD::AddChild(Path64) scales by the tree's scale (`ScalePath<double, int64_t>`).
            let tree = tree.map(|path| {
                path.iter()
                    .map(|p| p.with_xy(p.x as f64 * inv_scale, p.y as f64 * inv_scale))
                    .collect()
            });
            (tree, open)
        });
        self.base.clean_up();
        self.restore_z_callback();
        result
    }

    #[cfg(not(feature = "usingz"))]
    fn install_z_callback(&mut self) {}

    #[cfg(not(feature = "usingz"))]
    fn restore_z_callback(&mut self) {}

    /// `CheckCallback`: routes the engine's integer callback through the user's `f64` one.
    #[cfg(feature = "usingz")]
    fn install_z_callback(&mut self) {
        let Some(user) = self.z_callback.clone() else {
            self.base.z_callback = None;
            return;
        };
        let inv = self.inv_scale;
        let to_d = move |p: crate::Point64| p.with_xy(p.x as f64 * inv, p.y as f64 * inv);
        self.base.z_callback = Some(Box::new(
            move |e1b, e1t, e2b, e2t, pt: &mut crate::Point64| {
                let mut tmp = to_d(*pt);
                (user.borrow_mut())(to_d(e1b), to_d(e1t), to_d(e2b), to_d(e2t), &mut tmp);
                pt.z = tmp.z;
            },
        ));
    }

    #[cfg(feature = "usingz")]
    fn restore_z_callback(&mut self) {
        self.base.z_callback = None;
    }
}

/// `std::ilogb` for finite, non-zero values.
fn ilogb(v: f64) -> i32 {
    let exp = ((v.to_bits() >> 52) & 0x7ff) as i32;
    if exp == 0 {
        // subnormal
        (v * 2f64.powi(54)).log2().floor() as i32 - 54
    } else {
        exp - 1023
    }
}
