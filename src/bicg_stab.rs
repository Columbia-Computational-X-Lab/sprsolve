//! An impl of BiCGSTAB solver.

use super::{error::*, vecalg::*, MatVecMul};
use cauchy::Scalar;
use num_traits::{float::*, Zero};
use sprs::CsMatView;
use std::{
    intrinsics::{likely, unlikely},
    ptr::copy_nonoverlapping,
    slice::from_raw_parts_mut,
};

pub struct BiCGStab<'data, T: Scalar> {
    solver: BiCGStab_Backup<'data, T>,
}

impl<'data, T: Scalar> BiCGStab<'data, T> {
    #[allow(non_snake_case)]
    #[inline]
    pub fn new(A: CsMatView<'data, T>) -> SolveResult<Self> {
        Ok(BiCGStab {
            solver: BiCGStab_Backup::new(A)?,
        })
    }

    #[inline]
    pub fn solve(
        &mut self,
        rhs: &[T],
        x: &mut [T],
        max_iter: usize,
        tol: T::Real,
    ) -> SolveResult<(usize, T::Real)> {
        self.solver.solve(rhs, x, max_iter, tol)
    }
}

/// The backup implementation of BiCGSTAB algorithm when no BLAS/MKL is
/// available, focusing on correctness not performance.
#[allow(non_snake_case, non_camel_case_types)]
struct BiCGStab_Backup<'data, T: Scalar> {
    A: CsMatView<'data, T>,
    workspace: Vec<T>,
}

impl<'data, T: Scalar> BiCGStab_Backup<'data, T> {
    #[allow(non_snake_case)]
    pub fn new(A: CsMatView<'data, T>) -> SolveResult<Self> {
        if A.rows() != A.cols() {
            return Err(SolverError::IncompatibleMatrixFormat(String::from(
                "Not a square matrix",
            )));
        }

        if !A.is_csr() {
            return Err(SolverError::IncompatibleMatrixFormat(String::from(
                "Not in CSR format",
            )));
        }
        Ok(BiCGStab_Backup {
            A,
            workspace: vec![T::zero(); A.rows() * 6],
        })
    }

    /// Solves Ax = b, without preconditioner
    #[allow(clippy::many_single_char_names)]
    pub fn solve(
        &mut self,
        rhs: &[T],
        x: &mut [T],
        max_iter: usize,
        tol: T::Real,
    ) -> SolveResult<(usize, T::Real)> {
        let n = rhs.len();
        // check the format
        if n != self.A.rows() {
            return Err(SolverError::IncompatibleMatrixFormat(String::from(
                "Input vec dimension doesn't match the matrix size",
            )));
        }
        if n != x.len() {
            return Err(SolverError::IncompatibleMatrixFormat(String::from(
                "Input and output vec dimension do not match",
            )));
        }

        let rhs_norm = norm2(rhs);
        if unlikely(rhs_norm <= T::Real::epsilon()) {
            x.iter_mut().for_each(|v| *v = T::zero());
            return Ok((0, rhs_norm));
        }
        let tol2 = tol * rhs_norm;

        // Here is the internal memeory layout
        let ptr = self.workspace.as_mut_ptr();
        let r = unsafe { from_raw_parts_mut(ptr, n) }; // &mut [T]
        let r0 = unsafe { from_raw_parts_mut(ptr.add(n), n) };
        let s_z = unsafe { from_raw_parts_mut(ptr.add(2 * n), n) }; // s / z
        let y = unsafe { from_raw_parts_mut(ptr.add(3 * n), n) };
        let v = unsafe { from_raw_parts_mut(ptr.add(4 * n), n) };
        let t = unsafe { from_raw_parts_mut(ptr.add(5 * n), n) };
        unsafe {
            self.A.mul_vec_unchecked(x, &mut *r);
        }
        axpy(-T::one(), rhs, &mut *r); // r = A*x - rhs
        unsafe {
            // r0 = r
            copy_nonoverlapping(r.as_ptr(), r0.as_mut_ptr(), n);
        }
        let r0_norm = norm2(&*r0);
        if r0_norm <= tol2 {
            return Ok((0, r0_norm / rhs_norm));
        }
        let mut r0_norm_tol = r0_norm * T::Real::epsilon();
        r0_norm_tol = r0_norm_tol * r0_norm_tol;

        let mut w = T::one();
        // unroll the first iteration to initialize variables
        let mut rho = T::from_real(r0_norm * r0_norm); // rho != 0
        unsafe {
            // - y = r
            copy_nonoverlapping(r.as_ptr(), y.as_mut_ptr(), n);
            // - v = A*y
            self.A.mul_vec_unchecked(&*y, &mut *v);
        }
        // alpha = rho / r0.v
        let mut alpha = rho / conj_dot(&*r0, &*v);
        // - s = r
        unsafe {
            copy_nonoverlapping(r.as_ptr(), s_z.as_mut_ptr(), n);
        }
        // - z = s = r - alpha * v
        axpy(-alpha, &*v, &mut *s_z);
        // - t = A * z
        unsafe {
            self.A.mul_vec_unchecked(&*s_z, &mut *t);
        }
        // tmp = t.t
        let tmp = conj_dot(&*t, &*t);
        if likely(tmp.re() > T::Real::zero()) {
            // w = t.s/tmp
            w = conj_dot(&*t, &*s_z) / tmp;
        }
        // x = x - alpha*y - w*z
        axpy(-alpha, &*y, &mut *x);
        axpy(-w, &*s_z, &mut *x);
        // r = s
        unsafe {
            copy_nonoverlapping(s_z.as_ptr(), r.as_mut_ptr(), n);
        }
        // r = s - w * t
        axpy(-w, &*t, &mut *r);

        for its in 1..max_iter {
            let r_norm = norm2(&*r);
            if r_norm <= tol2 {
                return Ok((its, r_norm / rhs_norm));
            }
            let rho_old = rho;
            rho = conj_dot(&*r0, &*r);

            if unlikely(rho.abs() < r0_norm_tol) {
                // r = A*x
                unsafe {
                    self.A.mul_vec_unchecked(x, &mut *r);
                }
                // r = A*x - rhs
                axpy(-T::one(), rhs, &mut *r);
                // r0 = r
                unsafe {
                    copy_nonoverlapping(r.as_ptr(), r0.as_mut_ptr(), n);
                }
                rho = conj_dot(&*r, &*r);
                r0_norm_tol = rho.re() * T::Real::epsilon() * T::Real::epsilon();
            }
            let beta = (rho / rho_old) * (alpha / w);
            axpy(-w, &*v, &mut *y); // y - w*v
            scale(beta, &mut *y); // beta * (y-w*v)
            axpy(T::one(), &*r, &mut *y); // y = r + beta * (y - w*v)
                                          // - v = A*y
            unsafe {
                self.A.mul_vec_unchecked(&*y, &mut *v);
            }
            // alpha = rho / r0.v
            let tmp = conj_dot(&*r0, &*v);
            if unlikely(tmp.abs() <= T::Real::zero()) {
                //println!("{}", tmp);
                return Err(SolverError::BreakDown(its));
            }

            alpha = rho / tmp;
            // - s = r
            unsafe {
                copy_nonoverlapping(r.as_ptr(), s_z.as_mut_ptr(), n);
            }
            // - z = s = r - alpha * v
            axpy(-alpha, &*v, &mut *s_z);
            // - t = A * z
            unsafe {
                self.A.mul_vec_unchecked(&*s_z, &mut *t);
            }
            // tmp = t.t
            let tmp = conj_dot(&*t, &*t);
            if likely(tmp.re() > T::Real::zero()) {
                // w = t.s/tmp
                w = conj_dot(&*t, &*s_z) / tmp;
            }
            // x = x - alpha*y - w*z
            axpy(-alpha, &*y, &mut *x); // x - alpha * y
            axpy(-w, &*s_z, &mut *x); // x - alpha*y - w*z
                                      // r = s
            unsafe {
                copy_nonoverlapping(s_z.as_ptr(), r.as_mut_ptr(), n);
            }
            // r = s - w * t
            axpy(-w, &*t, &mut *r);
        }

        Err(SolverError::InsufficientIterNum(max_iter))
    }
}
