//! JNI bindings for `bark-core`.
//!
//! Exposes a `com.mrsobakin.bark.BarkPipeline` class with the following native
//! methods:
//!
//! ```java
//! class BarkPipeline {
//!     static native long nativeCreate(String configJson);
//!     static native void nativeDestroy(long handle);
//!     native void pushAudio(short[] frames);
//!     native String finalize();
//! }
//! ```

use jni::objects::JClass;
use jni::sys::{jlong, jshortArray, jstring};
use jni::JNIEnv;

use bark_core::{Bark, BarkConfig};

struct BarkInner {
    bark: Bark,
}

unsafe impl Send for BarkInner {}

fn box_handle(inner: BarkInner) -> jlong {
    Box::into_raw(Box::new(inner)) as jlong
}

fn deref_handle(handle: jlong) -> &'static mut BarkInner {
    unsafe { &mut *(handle as *mut BarkInner) }
}

fn drop_handle(handle: jlong) {
    unsafe {
        let _ = Box::from_raw(handle as *mut BarkInner);
    }
}

// ---------------------------------------------------------------------------
// JNI entry points
// ---------------------------------------------------------------------------

/// # Safety
/// `config_json` must be a valid JNI string reference.
#[no_mangle]
pub unsafe extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeCreate(
    mut env: JNIEnv,
    _class: JClass,
    config_json: jstring,
) -> jlong {
    let json: String = match env.get_string(&jni::objects::JString::from(
        jni::objects::JObject::from_raw(config_json),
    )) {
        Ok(s) => s.into(),
        Err(_) => {
            throw_illegal_argument(&mut env, "Failed to read config JSON string");
            return 0;
        }
    };

    let config: BarkConfig = match serde_json::from_str(&json) {
        Ok(c) => c,
        Err(e) => {
            throw_illegal_argument(&mut env, &format!("Invalid config JSON: {e}"));
            return 0;
        }
    };

    let bark = Bark::new(config);
    let inner = BarkInner { bark };
    box_handle(inner)
}

/// # Safety
/// `handle` must be a valid pointer returned by `nativeCreate`, or 0.
#[no_mangle]
pub unsafe extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeDestroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        drop_handle(handle);
    }
}

/// # Safety
/// `handle` must be a valid pointer. `frames` must be a valid JNI short array.
#[no_mangle]
pub unsafe extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativePushAudio(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    frames: jshortArray,
) {
    if handle == 0 {
        throw_illegal_state(&mut env, "BarkPipeline not initialized");
        return;
    }

    let inner = deref_handle(handle);

    let jarray = jni::objects::JShortArray::from_raw(frames);
    let len = env.get_array_length(&jarray).unwrap_or(0) as usize;

    if len == 0 {
        return;
    }

    let mut buf = vec![0i16; len];
    env.get_short_array_region(&jarray, 0, &mut buf).unwrap_or_else(|e| {
        eprintln!("bark-jni: get_short_array_region failed: {e}");
    });

    inner.bark.push_audio(&buf);
}

/// # Safety
/// `handle` must be a valid pointer. After this call, `handle` is consumed
/// and must not be used again.
#[no_mangle]
pub unsafe extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeFinalize(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    if handle == 0 {
        throw_illegal_state(&mut env, "BarkPipeline not initialized");
        return std::ptr::null_mut();
    }

    let inner = Box::from_raw(handle as *mut BarkInner);

    match inner.bark.finalize() {
        Ok(text) => match env.new_string(&text) {
            Ok(js) => js.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        Err(e) => {
            throw_illegal_state(&mut env, &format!("Finalize failed: {e}"));
            std::ptr::null_mut()
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn throw_illegal_argument(env: &mut JNIEnv, msg: &str) {
    let _ = env.throw_new("java/lang/IllegalArgumentException", msg);
}

fn throw_illegal_state(env: &mut JNIEnv, msg: &str) {
    let _ = env.throw_new("java/lang/IllegalStateException", msg);
}