//! Mirror native mod ABI v1. No Rust-owned values cross the library boundary.
use core::ffi::c_void;
pub const ABI_VERSION:u32=1;
pub const ENTRY_SYMBOL:&[u8]=b"mirror_mod_v1\0";
#[repr(C)]
#[derive(Clone,Copy)]
pub struct Slice {pub data:*const u8,pub len:usize}
impl Slice {
 pub fn new(bytes:&[u8])->Self{Self{data:bytes.as_ptr(),len:bytes.len()}}
 /// The caller must keep the memory alive for this call.
 pub unsafe fn bytes<'a>(self)->&'a [u8]{if self.len==0{&[]}else{unsafe{core::slice::from_raw_parts(self.data,self.len)}}}
}
#[repr(C)]
pub struct Buffer {pub data:*mut u8,pub len:usize,pub capacity:usize}
impl Buffer {
 pub fn new(mut bytes:Vec<u8>)->Self{let b=Self{data:bytes.as_mut_ptr(),len:bytes.len(),capacity:bytes.capacity()};core::mem::forget(bytes);b}
 pub fn empty()->Self{Self{data:core::ptr::null_mut(),len:0,capacity:0}}
}
/// Buffers are always released by the allocator that created them.
pub unsafe extern "C" fn free_buffer(b:Buffer){if b.capacity>0{unsafe{drop(Vec::from_raw_parts(b.data,b.len,b.capacity));}}}
pub type Invoke=unsafe extern "C" fn(*mut c_void,Slice,Slice,*mut Buffer)->i32;
#[repr(C)]
pub struct HostApi {pub abi:u32,pub invoke:Invoke,pub free_buffer:unsafe extern "C" fn(Buffer)}
#[repr(C)]
pub struct Api {
 pub abi:u32,
 pub query:Option<unsafe extern "C" fn(u32,*const f64,*mut f64)->bool>,
 pub create:unsafe extern "C" fn(Slice,*mut Buffer)->*mut c_void,
 pub call:unsafe extern "C" fn(*mut c_void,*const HostApi,*mut c_void,Slice,Slice,*mut Buffer)->i32,
 pub destroy:unsafe extern "C" fn(*mut c_void),
 pub free_buffer:unsafe extern "C" fn(Buffer),
}
unsafe impl Sync for Api {}
pub type Entry=unsafe extern "C" fn()->*const Api;

pub mod projection;

/// Current scalar world-query contracts. All arrays contain four doubles.
/// Unknown kinds return false. None of these callbacks may mutate state,
/// allocate, invoke the host, or retain input/output pointers.
pub mod query {
    pub const REALM_ACTIVE:u32=0; // xyz -> boolean in output[0]
    pub const REALM_HEIGHT:u32=1; // normal xyz -> height
    pub const REALM_DISTANCE:u32=2; // world xyz, terrain height -> air distance
    pub const REALM_COLOR:u32=3; // world xyz -> rgb
    pub const REALM_SHAFT:u32=4; // unused -> xyz
    pub const REALM_LEVELS:u32=5; // unused -> portal, floor
}
