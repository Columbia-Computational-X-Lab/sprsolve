use super::MatVecMul;
use cauchy::Scalar;
use mkl_sys::spblas as sp;
use num_complex::{Complex32, Complex64};
use sprs::CsMatI;
use std::{os::raw::c_int, result::Result};

const DEFAULT_SPARSE_MV_CALLS: i32 = 2000;

const COMPLEX32_ZERO: mkl_sys::MKL_Complex8 = mkl_sys::MKL_Complex8 { real: 0., imag: 0. };
const COMPLEX32_ONE: mkl_sys::MKL_Complex8 = mkl_sys::MKL_Complex8 { real: 1., imag: 0. };
const COMPLEX64_ZERO: mkl_sys::MKL_Complex16 = mkl_sys::MKL_Complex16 { real: 0., imag: 0. };
const COMPLEX64_ONE: mkl_sys::MKL_Complex16 = mkl_sys::MKL_Complex16 { real: 1., imag: 0. };

pub struct MklMat<T: Scalar> {
    // We have to use `u32` to be used with MKL interfaces
    _indptr: Vec<i32>,
    _indices: Vec<i32>,
    _data: Vec<T>,
    size: usize,
    sp_handle: sp::sparse_matrix_t,
}

impl<T: Scalar> MklMat<T> {
    /// Return the matrix size.
    #[inline(always)]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Create a general MKL Sparse Matrix from the privided [`CsMat`].
    pub fn new(m: CsMatI<T, i32>) -> Result<MklMat<T>, u32> {
        assert!(m.is_csr());

        let ncol = m.cols();
        let nrow = m.rows();
        assert_eq!(ncol, nrow);
        // We don't need this assert, which is gauranteed by the fact that index type is `i32`
        //assert!(ncol < c_int::max_value() as usize && nrow < c_int::max_value() as usize);

        let (indptr, indices, data) = m.into_raw_storage();
        let indptr_ptr = indptr.as_ptr();
        let mut sp_handle: sp::sparse_matrix_t = std::ptr::null_mut(); // *mut sparse_matrix
        macro_rules! create_csr {
            ($ty:ty, $func:ident, {$( $complex:ident )?}) => {
                if super::same_type::<T, $ty>() {
                    let status = unsafe {
                        sp::$func(
                            &mut sp_handle as *mut sp::sparse_matrix_t,
                            sp::sparse_index_base_t_SPARSE_INDEX_BASE_ZERO,
                            nrow as c_int,
                            ncol as c_int,
                            indptr_ptr as *mut i32,
                            indptr_ptr.add(1) as *mut i32,
                            indices.as_ptr() as *mut i32,
                            data.as_ptr() as *mut $ty $(as *mut mkl_sys::$complex )?,
                        )
                    };
                    if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
                        return Err(status);
                    }
                    let mret = MklMat { _indptr: indptr, _indices: indices, _data: data, size: nrow, sp_handle };
                    mret.mv_hint(DEFAULT_SPARSE_MV_CALLS)?;
                    return Ok(mret);
                }
            };
        }
        create_csr! {f32, mkl_sparse_s_create_csr, {}};
        create_csr! {f64, mkl_sparse_d_create_csr, {}};
        create_csr! {Complex32, mkl_sparse_c_create_csr, {MKL_Complex8}};
        create_csr! {Complex64, mkl_sparse_z_create_csr, {MKL_Complex16}};

        unreachable!();
    }

    /// Set the hint for matrix-vector multiplication and matrix-vector dot for performance optimization.
    ///
    /// It calls MKL routines to set both `mkl_sparse_set_mv_hint` and `mkl_sparse_dotmv_hint`,
    /// because sometimes both `mv` and `dotmv` are used in an iterative linear solver.
    #[inline]
    pub fn mv_and_dotmv_hint(&self, ncalls: i32) -> Result<(), u32> {
        debug_assert!(ncalls > 0);
        let descr = sp::matrix_descr {
            type_: sp::sparse_matrix_type_t_SPARSE_MATRIX_TYPE_GENERAL,
            mode: sp::sparse_fill_mode_t_SPARSE_FILL_MODE_FULL,
            diag: sp::sparse_diag_type_t_SPARSE_DIAG_NON_UNIT,
        };
        let status = unsafe {
            sp::mkl_sparse_set_mv_hint(
                self.sp_handle,
                sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                descr,
                DEFAULT_SPARSE_MV_CALLS,
            )
        };
        if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
            return Err(status);
        }

        let status = unsafe {
            sp::mkl_sparse_set_dotmv_hint(
                self.sp_handle,
                sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                descr,
                DEFAULT_SPARSE_MV_CALLS,
            )
        };
        if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
            return Err(status);
        }

        let status = unsafe { sp::mkl_sparse_optimize(self.sp_handle) };
        if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
            return Err(status);
        }
        Ok(())
    }

    /// Set the hint for matrix-vector multiplication for performance optimization.
    ///
    /// It calls MKL routines to set `mkl_sparse_set_mv_hint`,
    /// e.g., See [`BiCGStab`]
    #[inline]
    pub fn mv_hint(&self, ncalls: i32) -> Result<(), u32> {
        debug_assert!(ncalls > 0);
        let descr = sp::matrix_descr {
            type_: sp::sparse_matrix_type_t_SPARSE_MATRIX_TYPE_GENERAL,
            mode: sp::sparse_fill_mode_t_SPARSE_FILL_MODE_FULL,
            diag: sp::sparse_diag_type_t_SPARSE_DIAG_NON_UNIT,
        };
        let status = unsafe {
            sp::mkl_sparse_set_mv_hint(
                self.sp_handle,
                sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                descr,
                DEFAULT_SPARSE_MV_CALLS,
            )
        };
        if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
            return Err(status);
        }

        let status = unsafe { sp::mkl_sparse_optimize(self.sp_handle) };
        if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
            return Err(status);
        }
        Ok(())
    }
}

impl<T: Scalar> MatVecMul<T> for MklMat<T> {
    #[inline]
    fn mul_vec(&self, v_in: &[T], v_out: &mut [T]) {
        if self.size != v_in.len() || self.size != v_out.len() {
            panic!("Dimension mismatch");
        }
        unsafe {
            self.mul_vec_unchecked(v_in, v_out);
        }
    }

    #[inline]
    fn mul_vec_dot(&self, v_in: &[T], v_out: &mut [T]) -> T {
        if self.size != v_in.len() || self.size != v_out.len() {
            panic!("Dimension mismatch");
        }
        unsafe { self.mul_vec_dot_unchecked(v_in, v_out) }
    }

    unsafe fn mul_vec_unchecked(&self, v_in: &[T], v_out: &mut [T]) {
        let descr = sp::matrix_descr {
            type_: sp::sparse_matrix_type_t_SPARSE_MATRIX_TYPE_GENERAL,
            mode: sp::sparse_fill_mode_t_SPARSE_FILL_MODE_FULL,
            diag: sp::sparse_diag_type_t_SPARSE_DIAG_NON_UNIT,
        };
        macro_rules! sparse_mv {
            ($ty:ty, $func:ident) => {
                if super::same_type::<T, $ty>() {
                    let status = sp::$func(
                        sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                        1.,
                        self.sp_handle,
                        descr,
                        v_in.as_ptr() as *const $ty,
                        0.,
                        v_out.as_mut_ptr() as *mut $ty,
                    );
                    if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
                        panic!(format!(
                            "Cannot destroy MKL sparse matrix. Code = {}",
                            status
                        ));
                    }
                    return;
                }
            };
        }
        sparse_mv! {f32, mkl_sparse_s_mv};
        sparse_mv! {f64, mkl_sparse_d_mv};

        if super::same_type::<T, Complex32>() {
            let status = sp::mkl_sparse_c_mv(
                sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                COMPLEX32_ONE,
                self.sp_handle,
                descr,
                v_in.as_ptr() as *const Complex32 as *const mkl_sys::MKL_Complex8,
                COMPLEX32_ZERO,
                v_out.as_mut_ptr() as *mut Complex32 as *mut mkl_sys::MKL_Complex8,
            );
            if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
                panic!(format!(
                    "Cannot destroy MKL sparse matrix. Code = {}",
                    status
                ));
            }
            return;
        }

        if super::same_type::<T, Complex64>() {
            let status = sp::mkl_sparse_z_mv(
                sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                COMPLEX64_ONE,
                self.sp_handle,
                descr,
                v_in.as_ptr() as *const Complex64 as *const mkl_sys::MKL_Complex16,
                COMPLEX64_ZERO,
                v_out.as_mut_ptr() as *mut Complex64 as *mut mkl_sys::MKL_Complex16,
            );
            if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
                panic!(format!(
                    "Cannot destroy MKL sparse matrix. Code = {}",
                    status
                ));
            }
            return;
        }
        unreachable!();
    }

    #[inline]
    unsafe fn mul_vec_dot_unchecked(&self, v_in: &[T], v_out: &mut [T]) -> T {
        let descr = sp::matrix_descr {
            type_: sp::sparse_matrix_type_t_SPARSE_MATRIX_TYPE_GENERAL,
            mode: sp::sparse_fill_mode_t_SPARSE_FILL_MODE_FULL,
            diag: sp::sparse_diag_type_t_SPARSE_DIAG_NON_UNIT,
        };

        macro_rules! sparse_dotmv {
            ($ty:ty, $func:ident) => {
                if super::same_type::<T, $ty>() {
                    let mut d: $ty = 0.;
                    let status = sp::$func(
                        sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                        1.,
                        self.sp_handle,
                        descr,
                        v_in.as_ptr() as *const $ty,
                        0.,
                        v_out.as_mut_ptr() as *mut $ty,
                        &mut d as *mut $ty,
                    );
                    if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
                        panic!(format!(
                            "Cannot destroy MKL sparse matrix. Code = {}",
                            status
                        ));
                    }
                    return super::cast_as(&d);
                }
            };
        }
        sparse_dotmv! {f32, mkl_sparse_s_dotmv};
        sparse_dotmv! {f64, mkl_sparse_d_dotmv};

        if super::same_type::<T, Complex32>() {
            let mut d = T::zero();
            let status = sp::mkl_sparse_c_dotmv(
                sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                COMPLEX32_ONE,
                self.sp_handle,
                descr,
                v_in.as_ptr() as *const Complex32 as *const mkl_sys::MKL_Complex8,
                COMPLEX32_ZERO,
                v_out.as_mut_ptr() as *mut Complex32 as *mut mkl_sys::MKL_Complex8,
                &mut d as *mut T as *mut mkl_sys::MKL_Complex8,
            );
            if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
                panic!(format!(
                    "Cannot destroy MKL sparse matrix. Code = {}",
                    status
                ));
            }
            return super::cast_as(&d);
        }

        if super::same_type::<T, Complex64>() {
            let mut d = T::zero();
            let status = sp::mkl_sparse_z_dotmv(
                sp::sparse_operation_t_SPARSE_OPERATION_NON_TRANSPOSE,
                COMPLEX64_ONE,
                self.sp_handle,
                descr,
                v_in.as_ptr() as *const Complex64 as *const mkl_sys::MKL_Complex16,
                COMPLEX64_ZERO,
                v_out.as_mut_ptr() as *mut Complex64 as *mut mkl_sys::MKL_Complex16,
                &mut d as *mut T as *mut mkl_sys::MKL_Complex16,
            );
            if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
                panic!(format!(
                    "Cannot destroy MKL sparse matrix. Code = {}",
                    status
                ));
            }
            return super::cast_as(&d);
        }

        unreachable!();
    }
}

impl<T: Scalar> Drop for MklMat<T> {
    fn drop(&mut self) {
        // NOTE: Here we may need to ensure the handle sp_handle is dropped first
        let status = unsafe { sp::mkl_sparse_destroy(self.sp_handle) };
        if status != sp::sparse_status_t_SPARSE_STATUS_SUCCESS {
            panic!(format!(
                "Cannot destroy MKL sparse matrix. Code = {}",
                status
            ));
        }
    }
}

// -----------------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex64;
    use sprs::CsMatI;
    #[test]
    fn mkl_mat_vec() {
        let indptr: Vec<i32> = vec![0, 3, 3, 5, 6, 7];
        let indices: Vec<i32> = vec![1, 2, 3, 2, 3, 4, 4];
        let data = vec![
            0.75672424, 0.1649078, 0.30140296, 0.10358244, 0.6283315, 0.39244208, 0.57202407,
        ];

        let mat = CsMatI::new((5, 5), indptr, indices, data);
        mat.check_compressed_structure().unwrap();
        let mkl_mat = MklMat::new(mat).unwrap();

        let vector = vec![0.1, 0.2, -0.1, 0.3, 0.9];
        let mut res_vec = vec![0.; 5];
        mkl_mat.mul_vec(&vector, &mut res_vec);

        let expected_output = vec![0.22527496, 0., 0.17814121, 0.35319787, 0.51482166];

        let epsilon = 1e-8;

        assert!(res_vec
            .iter()
            .zip(expected_output.iter())
            .all(|(x, y)| (*x - *y).abs() < epsilon));
    }

    #[test]
    fn mkl_mat_vec_complex() {
        let indptr: Vec<i32> = vec![0, 3, 3, 5, 6, 7];
        let indices: Vec<i32> = vec![1, 2, 3, 2, 3, 4, 4];
        let data = vec![
            Complex64::new(0.75672424, 0.75672424),
            Complex64::new(0.1649078, 0.1649078),
            Complex64::new(0.30140296, 0.30140296),
            Complex64::new(0.10358244, 0.10358244),
            Complex64::new(0.6283315, 0.6283315),
            Complex64::new(0.39244208, 0.39244208),
            Complex64::new(0.57202407, 0.57202407),
        ];

        let mat = CsMatI::new((5, 5), indptr, indices, data);
        mat.check_compressed_structure().unwrap();
        let mkl_mat = MklMat::new(mat).unwrap();

        let vector = vec![
            Complex64::new(0.1, 0.),
            Complex64::new(0.2, 0.),
            Complex64::new(-0.1, 0.),
            Complex64::new(0.3, 0.),
            Complex64::new(0.9, 0.),
        ];
        let mut res_vec: Vec<Complex64> = vec![Default::default(); 5];
        mkl_mat.mul_vec(&vector, &mut res_vec);

        let expected_output = vec![0.22527496, 0., 0.17814121, 0.35319787, 0.51482166];

        //println!("{:?}", res_vec);
        //println!("{:?}", expected_output);
        let epsilon = 1e-8;

        assert!(res_vec
            .iter()
            .zip(expected_output.iter())
            .all(|(x, y)| (x.re - *y).abs() < epsilon && (x.im - *y).abs() < epsilon));
    }

    #[test]
    fn mkl_mat_vec_2() {
        let indptr: Vec<i32> = vec![0, 3, 5, 8, 11, 13];
        let indices: Vec<i32> = vec![0, 1, 3, 0, 1, 2, 3, 4, 0, 2, 3, 1, 4];
        let data = vec![
            1.0, -1.0, -3.0, -2.0, 5.0, 4.0, 6.0, 4.0, -4.0, 2.0, 7.0, 8.0, -5.0,
        ];

        let mat = CsMatI::new((5, 5), indptr, indices, data);
        mat.check_compressed_structure().unwrap();
        let mkl_mat = MklMat::new(mat).unwrap();

        let vector = vec![1.0, 5.0, 1.0, 4.0, 1.0];
        let mut res_vec = vec![0.; 5];
        mkl_mat.mul_vec(&vector, &mut res_vec);

        let expected = vec![-16.0, 23.0, 32.0, 26.0, 35.0];
        let epsilon = 1e-16;

        assert!(res_vec
            .iter()
            .zip(expected.iter())
            .all(|(x, y)| (*x - *y).abs() < epsilon));
    }

    #[test]
    fn mkl_mat_vec_dot_complex() {
        let indptr: Vec<i32> = vec![0, 3, 3, 5, 6, 7];
        let indices: Vec<i32> = vec![1, 2, 3, 2, 3, 4, 4];
        let data = vec![
            Complex64::new(0.75672424, 0.75672424),
            Complex64::new(0.1649078, 0.1649078),
            Complex64::new(0.30140296, 0.30140296),
            Complex64::new(0.10358244, 0.10358244),
            Complex64::new(0.6283315, 0.6283315),
            Complex64::new(0.39244208, 0.39244208),
            Complex64::new(0.57202407, 0.57202407),
        ];

        let mat = CsMatI::new((5, 5), indptr, indices, data);
        mat.check_compressed_structure().unwrap();
        let mkl_mat = MklMat::new(mat).unwrap();

        let vector = vec![
            Complex64::new(0.1, 0.),
            Complex64::new(0.2, 0.),
            Complex64::new(-0.1, 0.),
            Complex64::new(0.3, 0.),
            Complex64::new(0.9, 0.),
        ];
        let mut res_vec: Vec<Complex64> = vec![Default::default(); 5];
        let ret = mkl_mat.mul_vec_dot(&vector, &mut res_vec);
        let exp = super::super::vecalg::conj_dot(vector.as_slice(), res_vec.as_slice());

        approx::assert_abs_diff_eq!(exp.re, ret.re);
        approx::assert_abs_diff_eq!(exp.im, ret.im);
    }
}
