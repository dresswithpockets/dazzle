use std::ffi::CString;

pub(crate) fn string_to_cstring(string: String) -> CString {
    let mut vec: Vec<u8> = string.into_bytes();
    vec.push(0);
    CString::from_vec_with_nul(vec).expect("this should never fail")
}

pub(crate) fn str_to_cstring(string: &str) -> CString {
    CString::new(string).expect("this should never fail")
}
