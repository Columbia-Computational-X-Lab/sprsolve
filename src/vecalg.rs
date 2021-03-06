//! This module implements linear algebra operations on vectors.

use cauchy::Scalar;
#[cfg(feature = "mkl")]
use mkl_sys::blas::*;
use num_traits::Zero;
use std::ops::{Deref, DerefMut, Mul};

#[cfg(feature = "mkl")]
use std::ffi::c_void;
/// len of vector before we use blas
#[cfg(feature = "mkl")]
const DOT_BLAS_CUTOFF: usize = 64;
#[cfg(feature = "mkl")]
const SCALE_BLAS_CUTOFF: usize = 64;
#[cfg(feature = "mkl")]
const AXPY_BLAS_CUTOFF: usize = 64;

/// compute $\mathbf{x}\cdot\mathbf{y} = \mathbf{x}^T\mathbf{y}$.
///
/// **NOTE:** No conjugate is taken if the vector is complex-valued.
#[cfg(not(feature = "mkl"))]
#[inline]
pub fn dot<T, IN1, IN2>(vec1: IN1, vec2: IN2) -> T
where
    T: Scalar,
    IN1: Deref<Target = [T]>,
    IN2: Deref<Target = [T]>,
{
    assert_eq!(vec1[..].len(), vec2[..].len());
    dot_fallback(&vec1[..], &vec2[..])
}

/// compute $\mathbf{x}\cdot\mathbf{y} = \mathbf{x}^H\mathbf{y}$.
///
/// ```
/// # use sprsolve::vecalg::conj_dot;
/// use cauchy::c64;
///
/// let a = vec![c64::new(4., 3.); 100];
/// let b = vec![c64::new(2., -3.); 100];
/// let r = conj_dot(a.as_slice(), b.as_slice());
/// let t = a[0].conj() * b[0] * 100.;
/// approx::assert_abs_diff_eq!(t.re, r.re); //, epsilon = f64::EPSILON);
/// approx::assert_abs_diff_eq!(t.im, r.im);
/// ```
/// **NOTE:** If the vector is complex-valued, this function is conjugate-linear to
/// the first argument, and linear to the second argument.
#[cfg(not(feature = "mkl"))]
#[inline]
pub fn conj_dot<T, IN1, IN2>(vec1: IN1, vec2: IN2) -> T
where
    T: Scalar,
    IN1: Deref<Target = [T]>,
    IN2: Deref<Target = [T]>,
{
    assert_eq!(vec1[..].len(), vec2[..].len());
    conj_dot_fallback(&vec1[..], &vec2[..])
}

#[cfg(not(feature = "mkl"))]
#[inline]
pub fn norm2<T, VEC>(vec: VEC) -> T::Real
where
    T: Scalar,
    VEC: Deref<Target = [T]>,
{
    norm2_fallback(&vec[..])
}

/// Compute vec = vec * a
#[cfg(not(feature = "mkl"))]
#[inline]
pub fn scale<T, VEC>(a: T, mut vec: VEC)
where
    T: Scalar,
    VEC: DerefMut<Target = [T]>,
{
    scale_fallback(a, &mut vec[..]);
}

/// Compute vec = vec * a,
/// where a is a real number
#[cfg(not(feature = "mkl"))]
#[inline]
pub fn rscale<T, VEC>(a: T::Real, mut vec: VEC)
where
    T: Scalar,
    VEC: DerefMut<Target = [T]>,
{
    rscale_fallback(a, &mut vec[..]);
}

#[cfg(not(feature = "mkl"))]
#[inline]
pub fn conj<T, IN, OUT>(vec_in: IN, mut vec_out: OUT)
where
    T: Scalar,
    IN: Deref<Target = [T]>,
    OUT: DerefMut<Target = [T]>,
{
    assert_eq!(vec_in[..].len(), vec_out[..].len());
    conj_fallback(&vec_in[..], &mut vec_out[..])
}

/// The standard `axpy` operation as in BLAS: vec2 = vec2 + a*vec1
#[cfg(not(feature = "mkl"))]
#[inline]
pub fn axpy<S, T, IN, OUT>(a: S, vec1: IN, mut vec2: OUT)
where
    S: Copy,
    T: Scalar + Mul<S, Output = T>,
    IN: Deref<Target = [T]>,
    OUT: DerefMut<Target = [T]>,
{
    assert_eq!(vec1[..].len(), vec2[..].len());
    axpy_fallback(a, &vec1[..], &mut vec2[..])
}

/// The `axpby` operation as in MKL: vec2 = b*vec2 + a*vec1
///
/// # Example
///
/// ```
/// # use sprsolve::vecalg::axpby;
/// let a = vec![1_f32; 128];
/// let mut b = vec![2_f32; 128];
/// axpby(2., a.as_slice(), -1., b.as_mut_slice());
/// for i in 0..b.len() {
///     approx::assert_abs_diff_eq!(0., b[i]);
/// }
/// ```
#[cfg(not(feature = "mkl"))]
#[inline]
pub fn axpby<S, T, IN, OUT>(a: S, vec1: IN, b: S, mut vec2: OUT)
where
    S: Copy,
    T: Scalar + Mul<S, Output = T>,
    IN: Deref<Target = [T]>,
    OUT: DerefMut<Target = [T]>,
{
    assert_eq!(vec1[..].len(), vec2[..].len());
    axpby_fallback(a, &vec1[..], b, &mut vec2[..])
}

/// Dot product with CBLAS calls.
#[cfg(feature = "mkl")]
pub fn dot<T, IN1, IN2>(vec1: IN1, vec2: IN2) -> T
where
    T: Scalar,
    IN1: Deref<Target = [T]>,
    IN2: Deref<Target = [T]>,
{
    let n = vec1[..].len();
    assert_eq!(n, vec2[..].len());

    // Use only if the vector is large enough to be worth it
    if n > DOT_BLAS_CUTOFF && n < std::os::raw::c_int::max_value() as usize {
        macro_rules! dot {
            ($ty:ty, $func:ident, {}) => {
                if super::same_type::<T, $ty>() {
                    let v = unsafe {
                        $func(
                            n as i32, // this is safe because the assert! above
                            vec1[..].as_ptr() as *const $ty,
                            1,
                            vec2[..].as_ptr() as *const $ty,
                            1,
                        )
                    };
                    return super::cast_as::<$ty, T>(&v);
                }
            };
            ($ty:ty, $func:ident, {complex}) => {
                if super::same_type::<T, num_complex::Complex<$ty>>() {
                    let mut r: num_complex::Complex<$ty> = Default::default();
                    unsafe {
                        $func(
                            n as i32, // this is safe because the assert! above
                            vec1[..].as_ptr() as *const c_void,
                            1,
                            vec2[..].as_ptr() as *const c_void,
                            1,
                            &mut r as *mut num_complex::Complex<$ty> as *mut c_void,
                        );
                    }
                    return super::cast_as::<num_complex::Complex<$ty>, T>(&r);
                }
            };
        }

        dot! {f32, cblas_sdot, {}};
        dot! {f64, cblas_ddot, {}};
        dot! {f32, cblas_cdotu_sub, {complex} };
        dot! {f64, cblas_zdotu_sub, {complex} };
    }
    dot_fallback(&vec1[..], &vec2[..])
}

/// compute $\mathbf{x}\cdot\mathbf{y} = \mathbf{x}^H\mathbf{y}$.
///
/// **NOTE:** If the vector is complex-valued, this function is conjugate-linear to
/// the first argument, and linear to the second argument.
/// Here is an example to show the expected results.
/// ```
/// # use sprsolve::vecalg::conj_dot;
/// use cauchy::c64;
///
/// let a = vec![c64::new(4., 3.); 100];
/// let b = vec![c64::new(2., -3.); 100];
/// let r = conj_dot(a.as_slice(), b.as_slice());
/// let t = a[0].conj() * b[0] * 100.;
/// approx::assert_abs_diff_eq!(t.re, r.re); //, epsilon = f64::EPSILON);
/// approx::assert_abs_diff_eq!(t.im, r.im);
/// ```
#[cfg(feature = "mkl")]
pub fn conj_dot<T, IN1, IN2>(vec1: IN1, vec2: IN2) -> T
where
    T: Scalar,
    IN1: Deref<Target = [T]>,
    IN2: Deref<Target = [T]>,
{
    let n = vec1[..].len();
    assert_eq!(n, vec2[..].len());

    // Use only if the vector is large enough to be worth it
    if n > DOT_BLAS_CUTOFF && n < std::os::raw::c_int::max_value() as usize {
        macro_rules! dot {
            ($ty:ty, $func:ident, {}) => {
                if super::same_type::<T, $ty>() {
                    let v = unsafe {
                        $func(
                            n as i32, // this is safe because the assert! above
                            vec1[..].as_ptr() as *const $ty,
                            1,
                            vec2[..].as_ptr() as *const $ty,
                            1,
                        )
                    };
                    return super::cast_as::<$ty, T>(&v);
                }
            };
            ($ty:ty, $func:ident, {complex}) => {
                if super::same_type::<T, num_complex::Complex<$ty>>() {
                    let mut r: num_complex::Complex<$ty> = Default::default();
                    unsafe {
                        $func(
                            n as i32, // this is safe because the assert! above
                            vec1[..].as_ptr() as *const c_void,
                            1,
                            vec2[..].as_ptr() as *const c_void,
                            1,
                            &mut r as *mut num_complex::Complex<$ty> as *mut c_void,
                        );
                    }
                    return super::cast_as::<num_complex::Complex<$ty>, T>(&r);
                }
            };
        }

        dot! {f32, cblas_sdot, {}};
        dot! {f64, cblas_ddot, {}};
        dot! {f32, cblas_cdotc_sub, {complex} };
        dot! {f64, cblas_zdotc_sub, {complex} };
    }
    conj_dot_fallback(&vec1[..], &vec2[..])
}

#[cfg(feature = "mkl")]
pub fn norm2<T, VEC>(vec: VEC) -> T::Real
where
    T: Scalar,
    VEC: Deref<Target = [T]>,
{
    let n = vec[..].len();

    if n > DOT_BLAS_CUTOFF && n < std::os::raw::c_int::max_value() as usize {
        macro_rules! nrm2 {
            ($ty:ty, $func:ident, {}) => {
                if super::same_type::<T, $ty>() {
                    let v = unsafe { $func(n as i32, vec[..].as_ptr() as *const $ty, 1) };
                    return super::cast_as::<$ty, T::Real>(&v);
                }
            };
            ($ty:ty, $func:ident, {complex}) => {
                if super::same_type::<T, num_complex::Complex<$ty>>() {
                    let v = unsafe { $func(n as i32, vec[..].as_ptr() as *const c_void, 1) };
                    return super::cast_as::<$ty, T::Real>(&v);
                }
            };
        }
        nrm2! {f32, cblas_snrm2, {}};
        nrm2! {f64, cblas_dnrm2, {}};
        nrm2! {f32, cblas_scnrm2, {complex}};
        nrm2! {f64, cblas_dznrm2, {complex}};
    }
    norm2_fallback(&vec[..])
}

/// Compute vec = vec * a
#[cfg(feature = "mkl")]
pub fn scale<T, VEC>(a: T, mut vec: VEC)
where
    T: Scalar,
    VEC: DerefMut<Target = [T]>,
{
    let n = vec[..].len();
    if n > SCALE_BLAS_CUTOFF && n < std::os::raw::c_int::max_value() as usize {
        macro_rules! scale {
            ($ty:ty, $func:ident, {}) => {
                if super::same_type::<T, $ty>() {
                    unsafe {
                        $func(
                            n as i32, // this is safe because the assert! above
                            super::cast_as::<T, $ty>(&a),
                            vec[..].as_mut_ptr() as *mut $ty,
                            1,
                        )
                    };
                    return;
                }
            };
            ($ty:ty, $func:ident, {complex}) => {
                if super::same_type::<T, num_complex::Complex<$ty>>() {
                    unsafe {
                        $func(
                            n as i32, // this is safe because the assert! above
                            &a as *const T as *const c_void,
                            vec[..].as_mut_ptr() as *mut c_void,
                            1,
                        );
                    }
                    return;
                }
            };
        }
        scale! {f32, cblas_sscal, {}};
        scale! {f64, cblas_dscal, {}};
        scale! {f32, cblas_cscal, {complex}};
        scale! {f64, cblas_zscal, {complex}};
    }
    scale_fallback(a, &mut vec[..]);
}

/// Compute vec = vec * a
#[cfg(feature = "mkl")]
pub fn rscale<T, VEC>(a: T::Real, mut vec: VEC)
where
    T: Scalar,
    VEC: DerefMut<Target = [T]>,
{
    let n = vec[..].len();
    if n > SCALE_BLAS_CUTOFF && n < std::os::raw::c_int::max_value() as usize {
        macro_rules! scale {
            ($ty:ty, $func:ident, {}) => {
                if super::same_type::<T, $ty>() {
                    unsafe {
                        $func(
                            n as i32, // this is safe because the assert! above
                            super::cast_as::<T::Real, $ty>(&a),
                            vec[..].as_mut_ptr() as *mut $ty,
                            1,
                        )
                    };
                    return;
                }
            };
            ($ty:ty, $func:ident, {complex}) => {
                if super::same_type::<T, num_complex::Complex<$ty>>() {
                    unsafe {
                        $func(
                            n as i32, // this is safe because the assert! above
                            super::cast_as::<T::Real, $ty>(&a),
                            vec[..].as_mut_ptr() as *mut c_void,
                            1,
                        );
                    }
                    return;
                }
            };
        }
        scale! {f32, cblas_sscal, {}};
        scale! {f64, cblas_dscal, {}};
        scale! {f32, cblas_csscal, {complex}};
        scale! {f64, cblas_zdscal, {complex}};
    }
    rscale_fallback(a, &mut vec[..]);
}

#[cfg(feature = "mkl")]
#[inline]
pub fn conj<T, IN, OUT>(vec_in: IN, mut vec_out: OUT)
where
    T: Scalar,
    IN: Deref<Target = [T]>,
    OUT: DerefMut<Target = [T]>,
{
    let n = vec_in[..].len();
    assert_eq!(n, vec_out[..].len());
    if n > SCALE_BLAS_CUTOFF && n < std::os::raw::c_int::max_value() as usize {
        macro_rules! conj {
            ($ty:ty, $func:ident, {}) => {
                if super::same_type::<T, $ty>() {
                    unsafe {
                        $func(
                            n as i32,
                            vec_in[..].as_ptr() as *const $ty,
                            1,
                            vec_out[..].as_mut_ptr() as *mut $ty,
                            1,
                        );
                    }
                    return;
                }
            };
            ($ty:ty, $func:ident, {$func2:ident}) => {
                if super::same_type::<T, num_complex::Complex<$ty>>() {
                    unsafe {
                        $func(
                            n as i32,
                            vec_in[..].as_ptr() as *const c_void,
                            1,
                            vec_out[..].as_mut_ptr() as *mut c_void,
                            1,
                        );
                        let ptr = vec_out[..].as_mut_ptr() as *mut $ty;
                        $func2(n as i32, -1., ptr.add(1), 2);
                    }
                    return;
                }
            };
        }
        conj! {f32, cblas_scopy, {}};
        conj! {f64, cblas_dcopy, {}};
        conj! {f32, cblas_ccopy, {cblas_sscal}};
        conj! {f64, cblas_zcopy, {cblas_dscal}};
    }
    conj_fallback(&vec_in[..], &mut vec_out[..])
}

#[cfg(feature = "mkl")]
pub fn axpy<T, IN, OUT>(a: T, vec1: IN, mut vec2: OUT)
where
    T: Scalar,
    IN: Deref<Target = [T]>,
    OUT: DerefMut<Target = [T]>,
{
    let n = vec1[..].len();
    assert_eq!(n, vec2[..].len());
    if n > AXPY_BLAS_CUTOFF && n < std::os::raw::c_int::max_value() as usize {
        macro_rules! axpy {
            ($ty:ty, $func:ident, {}) => {
                if super::same_type::<T, $ty>() {
                    unsafe {
                        $func(
                            n as i32,
                            super::cast_as::<T, $ty>(&a),
                            vec1[..].as_ptr() as *const $ty,
                            1,
                            vec2[..].as_mut_ptr() as *mut $ty,
                            1,
                        );
                    }
                    return;
                }
            };
            ($ty:ty, $func:ident, {complex}) => {
                if super::same_type::<T, num_complex::Complex<$ty>>() {
                    unsafe {
                        $func(
                            n as i32,
                            &a as *const T as *const c_void,
                            vec1[..].as_ptr() as *const c_void,
                            1,
                            vec2[..].as_mut_ptr() as *mut c_void,
                            1,
                        );
                    }
                    return;
                }
            };
        }
        axpy! {f32, cblas_saxpy, {}};
        axpy! {f64, cblas_daxpy, {}};
        axpy! {f32, cblas_caxpy, {complex}};
        axpy! {f64, cblas_zaxpy, {complex}};
    }
    axpy_fallback(a, &vec1[..], &mut vec2[..])
}

/// The `axpby` operation as in MKL: vec2 = vec2 + a*vec1
///
/// # Example
///
/// ```
/// # use sprsolve::vecalg::axpby;
/// let a = vec![1_f32; 128];
/// let mut b = vec![2_f32; 128];
/// axpby(2., a.as_slice(), -1., b.as_mut_slice());
/// for i in 0..b.len() {
///     approx::assert_abs_diff_eq!(0., b[i]);
/// }
/// ```
#[cfg(feature = "mkl")]
pub fn axpby<T, IN, OUT>(a: T, vec1: IN, b: T, mut vec2: OUT)
where
    T: Scalar,
    IN: Deref<Target = [T]>,
    OUT: DerefMut<Target = [T]>,
{
    let n = vec1[..].len();
    assert_eq!(n, vec2[..].len());
    if n > AXPY_BLAS_CUTOFF && n < std::os::raw::c_int::max_value() as usize {
        macro_rules! axpy {
            ($ty:ty, $func:ident, {}) => {
                if super::same_type::<T, $ty>() {
                    unsafe {
                        $func(
                            n as i32,
                            super::cast_as::<T, $ty>(&a),
                            vec1[..].as_ptr() as *const $ty,
                            1,
                            super::cast_as::<T, $ty>(&b),
                            vec2[..].as_mut_ptr() as *mut $ty,
                            1,
                        );
                    }
                    return;
                }
            };
            ($ty:ty, $func:ident, {complex}) => {
                if super::same_type::<T, num_complex::Complex<$ty>>() {
                    unsafe {
                        $func(
                            n as i32,
                            &a as *const T as *const c_void,
                            vec1[..].as_ptr() as *const c_void,
                            1,
                            &b as *const T as *const c_void,
                            vec2[..].as_mut_ptr() as *mut c_void,
                            1,
                        );
                    }
                    return;
                }
            };
        }
        axpy! {f32, cblas_saxpby, {}};
        axpy! {f64, cblas_daxpby, {}};
        axpy! {f32, cblas_caxpby, {complex}};
        axpy! {f64, cblas_zaxpby, {complex}};
    }
    axpby_fallback(a, &vec1[..], b, &mut vec2[..])
}

#[inline]
fn dot_fallback<T: Scalar>(vec1: &[T], vec2: &[T]) -> T {
    vec1.iter()
        .zip(vec2.iter())
        .fold(T::zero(), |acc, (x, y)| acc + (*x) * (*y))
}

#[inline]
fn conj_dot_fallback<T: Scalar>(vec1: &[T], vec2: &[T]) -> T {
    vec1.iter()
        .zip(vec2.iter())
        .fold(T::zero(), |acc, (x, y)| acc + x.conj() * (*y))
}

#[inline]
fn axpy_fallback<S: Copy, T: Scalar + Mul<S, Output = T>>(a: S, vec1: &[T], vec2: &mut [T]) {
    vec2.iter_mut()
        .zip(vec1.iter())
        .for_each(|(y, x)| *y += *x * a);
}

#[inline]
fn conj_fallback<T: Scalar>(vec_in: &[T], vec_out: &mut [T]) {
    vec_out
        .iter_mut()
        .zip(vec_in.iter())
        .for_each(|(y, x)| *y = x.conj());
}

#[inline]
fn axpby_fallback<S: Copy, T: Scalar + Mul<S, Output = T>>(a: S, vec1: &[T], b: S, vec2: &mut [T]) {
    vec2.iter_mut()
        .zip(vec1.iter())
        .for_each(|(y, x)| *y = *x * a + *y * b);
}

#[inline(always)]
fn scale_fallback<T: Scalar>(a: T, vec: &mut [T]) {
    vec.iter_mut().for_each(|v| *v *= a);
}
#[inline(always)]
fn rscale_fallback<T: Scalar>(a: T::Real, vec: &mut [T]) {
    vec.iter_mut().for_each(|v| *v = v.mul_real(a));
}

#[inline]
fn norm2_fallback<T: Scalar>(vec: &[T]) -> T::Real {
    let v = vec.iter().fold(T::Real::zero(), |acc, x| acc + x.square());
    v.sqrt()
}

// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_norm2() {
        let a = vec![1.; 25];
        let b = norm2(&a[..]);
        approx::assert_abs_diff_eq!(5.0, b);

        let a = vec![1.; 100];
        let b = norm2(&a[..]);
        approx::assert_abs_diff_eq!(10.0, b);

        use cauchy::c64;
        let a = vec![c64::new(1., 1.); 50];
        let b = norm2(&a[..]);
        approx::assert_abs_diff_eq!(10.0, b);
    }

    #[test]
    fn test_dot_generic() {
        let a: &[f64] = &[1., 1., 1., 1., 1., 1.];
        let b: &[f64] = &[1., 2., 3., 4., 5., 6.];
        approx::assert_abs_diff_eq!(21., dot_fallback(a, b));

        let a = vec![1.; 6];
        let b = vec![1., 2., 3., 4., 5., 6.];
        approx::assert_abs_diff_eq!(21., dot_fallback(&a, &b));
        println!("{:?}", b);
    }

    #[test]
    fn test_conj_dot() {
        let a = vec![1_f64; 100];
        let b = vec![2_f64; 100];
        let r = conj_dot(a.as_slice(), b.as_slice());
        approx::assert_abs_diff_eq!(200., r); //, epsilon = f64::EPSILON);

        let a = vec![2_f32; 100];
        let b = vec![3_f32; 100];
        let r = conj_dot(a.as_slice(), b.as_slice());
        approx::assert_abs_diff_eq!(600_f32, r); //, epsilon = f64::EPSILON);

        use cauchy::c32;
        let a = vec![c32::new(2., 3.); 100];
        let b = vec![c32::new(2., -3.); 100];
        let r = conj_dot(a.as_slice(), b.as_slice());
        let t = a[0].conj() * b[0] * 100.;
        approx::assert_abs_diff_eq!(t.re, r.re); //, epsilon = f64::EPSILON);
        approx::assert_abs_diff_eq!(t.im, r.im);
    }

    #[test]
    fn test_scale() {
        let mut a = vec![1_f64; 100];
        scale(1.5, a.as_mut_slice());
        for x in &a {
            approx::assert_abs_diff_eq!(1.5, *x);
        }

        use cauchy::c32;
        let mut a = vec![c32::new(2., 3.); 100];
        let s = c32::new(1.2, 4.8);
        let v = a[0] * s;
        scale(s, a.as_mut_slice());
        for x in &a {
            approx::assert_abs_diff_eq!(v.re, x.re);
            approx::assert_abs_diff_eq!(v.im, x.im);
        }
    }

    #[test]
    fn test_dot() {
        let a = vec![1_f64; 100];
        let b = vec![2_f64; 100];
        let r = dot(a.as_slice(), b.as_slice());
        approx::assert_abs_diff_eq!(200., r); //, epsilon = f64::EPSILON);

        let a = vec![2_f32; 100];
        let b = vec![3_f32; 100];
        let r = dot(a.as_slice(), b.as_slice());
        approx::assert_abs_diff_eq!(600_f32, r); //, epsilon = f64::EPSILON);

        use cauchy::c64;
        let a = vec![c64::new(2., 0.); 100];
        let b = vec![c64::new(2.5, 0.); 100];
        let r = dot(a.as_slice(), b.as_slice());
        approx::assert_abs_diff_eq!(500., r.re); //, epsilon = f64::EPSILON);
        approx::assert_abs_diff_eq!(0., r.im);

        let a = vec![c64::new(2., 1.); 100];
        let b = vec![c64::new(3., 1.); 100];
        let r = dot(a.as_slice(), b.as_slice());
        let t = a[0] * b[0] * 100.;
        approx::assert_abs_diff_eq!(t.re, r.re); //, epsilon = f64::EPSILON);
        approx::assert_abs_diff_eq!(t.im, r.im);

        use cauchy::c32;
        let a = vec![c32::new(2., 0.); 100];
        let b = vec![c32::new(2.5, 0.); 100];
        let r = dot(a.as_slice(), b.as_slice());
        approx::assert_abs_diff_eq!(500., r.re); //, epsilon = f64::EPSILON);
        approx::assert_abs_diff_eq!(0., r.im);

        let a = vec![c32::new(2., 3.); 100];
        let b = vec![c32::new(2., -3.); 100];
        let r = dot(a.as_slice(), b.as_slice());
        approx::assert_abs_diff_eq!((2. * 2. + 3. * 3.) * 100., r.re); //, epsilon = f64::EPSILON);
        approx::assert_abs_diff_eq!(0., r.im);
    }

    #[test]
    fn dot_generic_complex() {
        use cauchy::c64;
        let a = vec![c64::new(0., 1.); 6];
        let mut b: Vec<c64> = Vec::with_capacity(a.len());
        for i in 0..a.len() {
            b.push(c64::new(i as f64, 0.));
        }
        let ret = dot_fallback(&a, &b);
        approx::assert_abs_diff_eq!(0., ret.re);
        approx::assert_abs_diff_eq!(15., ret.im);

        const N: usize = 8;
        let mut a: Vec<c64> = Vec::with_capacity(N);
        let mut b: Vec<c64> = Vec::with_capacity(N);
        let mut s = 0_f64;
        for i in 0..N {
            a.push(c64::new(i as f64, i as f64));
            b.push(c64::new(i as f64, -(i as f64)));
            s += (i * i * 2) as f64;
        }
        let ret = dot_fallback(&a, &b);
        approx::assert_abs_diff_eq!(s, ret.re);
        approx::assert_abs_diff_eq!(0., ret.im);
    }

    #[test]
    fn axpy_generic_complex() {
        use cauchy::c64;
        let a = vec![c64::new(0., 1.); 6];
        let mut b: Vec<c64> = Vec::with_capacity(a.len());
        for i in 0..a.len() {
            b.push(c64::new(i as f64, 0.));
        }
        axpy_fallback(1., &a, &mut b);
        for i in 0..a.len() {
            approx::assert_abs_diff_eq!(i as f64, b[i].re);
            approx::assert_abs_diff_eq!(1., b[i].im);
        }

        let mut b: Vec<c64> = Vec::with_capacity(a.len());
        for i in 0..a.len() {
            b.push(c64::new(i as f64, 0.));
        }
        axpy_fallback(c64::new(0., 1.), &a, &mut b);
        for i in 0..a.len() {
            approx::assert_abs_diff_eq!((i as f64) - 1., b[i].re);
            approx::assert_abs_diff_eq!(0., b[i].im);
        }
    }

    #[test]
    fn axpy_f32() {
        let a = vec![1_f32; 128];
        let mut b = vec![0_f32; 128];
        for _ in 0..4 {
            axpy(2_f32, a.as_slice(), b.as_mut_slice());
        }
        for i in 0..a.len() {
            approx::assert_abs_diff_eq!(8., b[i]);
        }
    }
    #[test]
    fn axpby_f32() {
        let a = vec![1_f32; 128];
        let mut b = vec![0_f32; 128];
        for _ in 0..4 {
            axpby(2_f32, a.as_slice(), 1_f32, b.as_mut_slice());
        }
        for i in 0..a.len() {
            approx::assert_abs_diff_eq!(8., b[i]);
        }
        for _ in 0..3 {
            axpby(2_f32, a.as_slice(), -1_f32, b.as_mut_slice());
        }
        for i in 0..a.len() {
            approx::assert_abs_diff_eq!(-6., b[i]);
        }
    }
    #[test]
    fn test_conj() {
        use cauchy::c64;
        let a = vec![c64::new(1., 1.); 100];
        let mut b = vec![c64::new(1., 1.); 100];
        conj_fallback(&a, &mut b);
        for (x, y) in b.iter().zip(a.iter()) {
            approx::assert_abs_diff_eq!(x.im, -y.im);
        }
        let a = vec![c64::new(3., 2.); 100];
        conj(a.as_slice(), b.as_mut_slice());
        //println!("{:?}", b);
        for (x, y) in b.iter().zip(a.iter()) {
            approx::assert_abs_diff_eq!(x.im, -y.im);
        }
    }
    #[test]
    fn test_conj2() {
        use cauchy::c32;
        let a = vec![c32::new(1., 1.); 100];
        let mut b = vec![c32::new(1., 1.); 100];
        conj_fallback(&a, &mut b);
        for (x, y) in b.iter().zip(a.iter()) {
            approx::assert_abs_diff_eq!(x.im, -y.im);
        }
        let a = vec![c32::new(3., 2.); 100];
        conj(a.as_slice(), b.as_mut_slice());
        //println!("{:?}", b);
        for (x, y) in b.iter().zip(a.iter()) {
            approx::assert_abs_diff_eq!(x.im, -y.im);
        }
    }
    #[test]
    fn test_rscale() {
        use cauchy::c64;
        let mut b = vec![c64::new(1., 2.); 100];
        rscale(9., b.as_mut_slice());
        //println!("{:?}", b);
        for x in b.iter() {
            approx::assert_abs_diff_eq!(x.re, 9.);
            approx::assert_abs_diff_eq!(x.im, 18.);
        }
    }
}
