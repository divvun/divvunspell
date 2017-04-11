use libc::size_t;
use std::str;
use std::mem;

#[repr(C)]
pub struct CVec<T> {
    pub len: size_t,
    pub capacity: size_t,
    pub ptr: *mut T
}

pub trait IntoCVec {
    type Item;

    unsafe fn into_c_vec_raw(self) -> *mut CVec<Self::Item>;
    unsafe fn from_c_vec_raw(ptr: *mut CVec<Self::Item>) -> Vec<Self::Item>;
}

impl<T> IntoCVec for Vec<T> {
    type Item = T;

    unsafe fn into_c_vec_raw(mut self) -> *mut CVec<Self::Item> {
        self.shrink_to_fit();

        let p = self.as_mut_ptr();
        let len = self.len();
        let cap = self.capacity();

        let c_vec = Box::new(CVec {
            len: len,
            capacity: cap,
            ptr: p
        });

        mem::forget(self);

        Box::into_raw(c_vec) as *mut CVec<Self::Item>
    }

    unsafe fn from_c_vec_raw(ptr: *mut CVec<Self::Item>) -> Vec<Self::Item> {
        let c_vec = Box::from_raw(&mut *ptr);
        
        Vec::from_raw_parts(c_vec.ptr, c_vec.len, c_vec.capacity)
    }
}

impl<T> Drop for CVec<T> {
    fn drop(&mut self) {
        println!("Dropped cvec");
    }
}

#[test]
fn test_convert_vec() {
    let test = vec!["this".to_string(), "works".to_string()];

    unsafe {
        let c_vec = test.into_c_vec_raw();
        assert!((*c_vec).len == 2);
    }
}