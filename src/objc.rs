//! Just enough Objective-C runtime to build a menu. Never used on the keystroke path.
//! Arguments must have the exact C type of the method (`isize` for NSInteger, `f64` for CGFloat).

use std::ffi::{CStr, CString, c_char, c_void};

pub type Id = *mut c_void;
pub type Sel = *const c_void;

#[link(name = "AppKit", kind = "framework")]
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    fn objc_allocateClassPair(superclass: Id, name: *const c_char, extra: usize) -> Id;
    fn objc_registerClassPair(cls: Id);
    fn class_addMethod(cls: Id, sel: Sel, imp: *const c_void, types: *const c_char) -> bool;
    pub fn objc_autoreleasePoolPush() -> *mut c_void;
    pub fn objc_autoreleasePoolPop(pool: *mut c_void);
}

pub fn class(name: &CStr) -> Id {
    unsafe { objc_getClass(name.as_ptr()) }
}

pub fn sel(name: &CStr) -> Sel {
    unsafe { sel_registerName(name.as_ptr()) }
}

macro_rules! send {
    ($name:ident $(, $a:ident: $t:ident)*) => {
        pub unsafe fn $name<$($t,)* R>(o: Id, s: &CStr $(, $a: $t)*) -> R {
            unsafe {
                let f: unsafe extern "C" fn(Id, Sel $(, $t)*) -> R = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                f(o, sel(s) $(, $a)*)
            }
        }
    };
}
send!(send0);
send!(send1, a: A);
send!(send2, a: A, b: B);
send!(send3, a: A, b: B, c: C);

/// An autoreleased NSString.
pub fn ns(s: &str) -> Id {
    let c = CString::new(s).unwrap_or_default();
    unsafe { send1(class(c"NSString"), c"stringWithUTF8String:", c.as_ptr()) }
}

/// Registers an NSObject subclass with the given methods, each (selector, function,
/// Objective-C type encoding), and returns one instance of it.
pub fn object(name: &CStr, methods: &[(&CStr, *const c_void, &CStr)]) -> Id {
    unsafe {
        let cls = objc_allocateClassPair(class(c"NSObject"), name.as_ptr(), 0);
        for (s, f, types) in methods {
            class_addMethod(cls, sel(s), *f, types.as_ptr());
        }
        objc_registerClassPair(cls);
        send0(send0(cls, c"alloc"), c"init")
    }
}
