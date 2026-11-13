//! Best-effort priority classification for latency-sensitive preparation workers.
//! Called once by the new worker itself, never from native audio callbacks.
//! Apple SDK pthread/qos.h: zero accepts the request; other results are errno.
pub(crate) const NOT_REQUESTED: i32 = i32::MIN;
#[cfg(any(target_os="macos",target_os="ios"))]
unsafe extern "C" { fn pthread_set_qos_class_self_np(class:u32,relative_priority:i32)->i32; }
pub(crate) fn request() -> i32 {
    #[cfg(any(target_os="macos",target_os="ios"))]
    { unsafe { pthread_set_qos_class_self_np(0x21,0) } }
    #[cfg(not(any(target_os="macos",target_os="ios")))]
    { NOT_REQUESTED }
}
pub(crate) fn status(result:i32)->serde_json::Value {
    let supported=cfg!(any(target_os="macos",target_os="ios"));
    serde_json::json!({"supported":supported,"requested_class":if supported{"user_interactive"}else{"platform_default"},
        "requested":result!=NOT_REQUESTED,"accepted":result==0,
        "error_code":if result!=NOT_REQUESTED&&result!=0{Some(result)}else{None},"realtime":false})
}
