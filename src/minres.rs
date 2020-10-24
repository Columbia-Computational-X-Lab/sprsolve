//! An impl of MINRES algorithm for linear sparse solve.

use super::{error::*, vecalg::*, MatVecMul};
use cauchy::Scalar;
use num_traits::{float::*, One, Zero};
use std::{intrinsics::unlikely, ptr::copy_nonoverlapping, slice::from_raw_parts_mut};

/// **NOTE:** This MINRES solver works only for real-valued symmetric systems or
/// complex-valued Hermitian system. The system can be indefinite.
///
/// **Note:** This class won't check if the input matrix is hermitian.
#[allow(non_snake_case)]
pub struct MinRes<'data, T: Scalar, M: MatVecMul<T>> {
    A: &'data M,
    workspace: Vec<T>,
    size: usize,
}

impl<'data, T: Scalar, M: MatVecMul<T>> MinRes<'data, T, M> {
    #[allow(non_snake_case)]
    pub fn new(A: &'data M, size: usize) -> Self {
        MinRes {
            A,
            workspace: vec![T::zero(); size * 6],
            size,
        }
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
        if n != self.size {
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
            // when rhs = 0, x is set to zero.
            x.iter_mut().for_each(|v| *v = T::zero());
            return Ok((0, rhs_norm));
        }
        let threshold = tol * rhs_norm;

        // initialize
        let mut c = T::one();
        let mut c_old = T::one();
        let mut s = T::Real::zero();
        let mut s_old = T::Real::zero();
        let mut eta = T::one();

        // set vectors using preallocated memeory
        let ptr = self.workspace.as_mut_ptr();
        let mut v_old = unsafe { from_raw_parts_mut(ptr, n) }; // &mut [T]
        let mut v_new = unsafe { from_raw_parts_mut(ptr.add(n), n) };
        let mut v = unsafe { from_raw_parts_mut(ptr.add(2 * n), n) };
        let mut p_old = unsafe { from_raw_parts_mut(ptr.add(3 * n), n) };
        let mut p_oold = unsafe { from_raw_parts_mut(ptr.add(4 * n), n) };
        let mut p = unsafe { from_raw_parts_mut(ptr.add(5 * n), n) };

        // initialize v and v_new
        unsafe {
            copy_nonoverlapping(rhs.as_ptr(), v_new.as_mut_ptr(), n); // v_new = rhs
            self.A.mul_vec_unchecked(x, &mut *v_old); // v_old = A * x
        }
        axpy(-T::one(), &*v_old, &mut *v_new); // v_new = rhs - A*x
        let mut res_norm = norm2(&*v_new);
        let mut beta_new = res_norm;
        let beta_one = beta_new;
        scale(T::from_real(T::Real::one() / beta_new), &mut *v_new);

        v.iter_mut().for_each(|t| *t = T::zero()); // v = zero
        p_old.iter_mut().for_each(|t| *t = T::zero()); // p_old = zero
        p.iter_mut().for_each(|t| *t = T::zero()); // p = zero

        for its in 0..max_iter {
            let beta = beta_new;
            let v_t_ptr = v_old.as_mut_ptr();
            // Here we just move the pointers to avoid memory copy
            v_old = unsafe { from_raw_parts_mut(v.as_mut_ptr(), n) }; // v_old <- v
            v = unsafe { from_raw_parts_mut(v_new.as_mut_ptr(), n) }; // v <- v_new
            v_new = unsafe { from_raw_parts_mut(v_t_ptr, n) }; // v_new <- v_old

            /*
            unsafe {
                self.A.mul_vec_unchecked(v, v_new); // v_new = A*v
            }
            // v_new = A*v - beta*v_old
            axpy(T::from_real(-beta), &*v_old, &mut *v_new);  // >>> A*q_k - beta_{k-1} q_{k-1}

            // compute the new Lanczos vector
            // See P. 562 of Matrix Computation Ed.4
            // alpha = (A*v - beta * v_old).v
            // >>> beta is now beta_{k-1}
            let alpha = conj_dot(&*v_new, &*v); // v_new . v                >>> alpha is alpha_k
            axpy(-alpha, &*v, &mut *v_new); // v_new -= alpha * v           >>> v_new is now r_k
            */
            // According to the Wiki (https://en.wikipedia.org/wiki/Lanczos_algorithm)
            // This order of computing Lanczos vectors is the most numerically stable.
            // comptue v_new = A * v
            //         alpha = conj(v).v_new
            let alpha = unsafe { self.A.mul_vec_dot_unchecked(v, v_new) };
            axpy(T::from_real(-beta), &*v_old, &mut *v_new); // >>> A*q_k - beta_{k-1} q_{k-1}
            axpy(-alpha, &*v, &mut *v_new); // v_new = A*q_k - beta_{k-1}q_{k-1} - alpha*q_k  >>> v_new is now r_k

            beta_new = norm2(&*v_new); // beta_new = |v_new|                >>> beta_new is beta_k
            scale(T::from_real(T::Real::one() / beta_new), &mut *v_new); // >>> v_new is now q_k+1

            // --- Givens rotation ---
            // G^T_{k-1} = [ c_old  s_old ]
            //             [-s_old  c_old ]
            // ---------------------------------
            // [ r3 ] = G^T_{k-2} [ 0          ]
            // [ tr ]             [ beta_{k-1} ]
            // ---------------------------------
            // [ r2      ] = G^T_{k-2} G^T_{k-1} [ tr ]
            // [ r1_hat  ]                       [ alpha_{k} ]
            let r3 = s_old * beta; // s, s_old, c and c_old are still from previous iteration
            let tr = c_old.mul_real(beta);
            let r2 = alpha.mul_real(s) + c * tr; // s, s_old, c and c_old are still from previous iteration
                                                 // previous two Givens rotation applied to [0 beta_{k-1} alpha_k] -> [x c*beta_{k-1}]
            let r1_hat = c * alpha - tr.mul_real(s);

            // now need to construct Givens rotation for [r1_hat beta_k]
            let r1 = num_traits::Float::sqrt(r1_hat.square() + beta_new.square());

            c_old = c; // store for next iteration
            s_old = s; // store for next iteration
                       // [ c  s ]
                       // [-s  c ]
            c = r1_hat.div_real(r1); // new cosine
            s = beta_new / r1; // new sine

            // Update solution
            let p_t_ptr = p_oold.as_mut_ptr();
            p_oold = unsafe { from_raw_parts_mut(p_old.as_mut_ptr(), n) }; // p_oold <- p_old
            p_old = unsafe { from_raw_parts_mut(p.as_mut_ptr(), n) }; // p_old <- p
            p = unsafe { from_raw_parts_mut(p_t_ptr, n) };
            unsafe {
                copy_nonoverlapping(v.as_ptr(), p.as_mut_ptr(), n); // p = v
            }
            axpy(-r2, &*p_old, &mut *p); // p = v - r2*p_old
            axpy(T::from_real(-r3), &*p_oold, &mut *p); // p = v - r2*p_old - r3*p_oold
            scale(T::from_real(T::Real::one() / r1), &mut *p);

            axpy((c * eta).mul_real(beta_one), &*p, &mut *x); //  x += beta_one*c*eta*p

            res_norm *= num_traits::Float::abs(s);
            if res_norm < threshold {
                return Ok((its, res_norm / rhs_norm));
            }
            eta = eta.mul_real(-s);
        }

        Err(SolverError::InsufficientIterNum(max_iter))
    }
}
