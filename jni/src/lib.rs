use jni::objects::{JClass, JShortArray, JString};
use jni::sys::{jlong, jshortArray, jstring};
use jni::JNIEnv;

use bark_core::{Bark, BarkConfig};

fn box_handle(bark: Bark) -> jlong {
    Box::into_raw(Box::new(bark)) as jlong
}

unsafe fn deref_handle(handle: jlong) -> &'static mut Bark {
    &mut *(handle as *mut Bark)
}

unsafe fn drop_handle(handle: jlong) {
    let _ = Box::from_raw(handle as *mut Bark);
}

fn throw_new(env: &mut JNIEnv, cls: &str, msg: &str) {
    let _ = env.throw_new(cls, msg);
}

fn throw_arg(env: &mut JNIEnv, msg: &str) {
    throw_new(env, "java/lang/IllegalArgumentException", msg);
}

fn throw_state(env: &mut JNIEnv, msg: &str) {
    throw_new(env, "java/lang/IllegalStateException", msg);
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeCreate(
    mut env: JNIEnv,
    _class: JClass,
    config_json: jstring,
) -> jlong {
    let json: String = match unsafe { env.get_string(&JString::from_raw(config_json)) } {
        Ok(s) => s.into(),
        Err(_) => {
            throw_arg(&mut env, "Failed to read config JSON string");
            return 0;
        }
    };

    let config: BarkConfig = match serde_json::from_str(&json) {
        Ok(c) => c,
        Err(e) => {
            throw_arg(&mut env, &format!("Invalid config JSON: {e}"));
            return 0;
        }
    };

    let bark = match Bark::new(config) {
        Ok(b) => b,
        Err(e) => {
            throw_state(&mut env, &format!("BarkPipeline init failed: {e}"));
            return 0;
        }
    };

    box_handle(bark)
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeDestroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        // SAFETY: handle was produced by box_handle and hasn't been freed yet
        // (the Java-side close() guarantees single-call semantics).
        unsafe { drop_handle(handle) }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeReset(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        throw_state(&mut env, "BarkPipeline not initialized");
        return;
    }
    // SAFETY: see deref_handle's safety contract — serialised JNI calls.
    let bark = unsafe { deref_handle(handle) };
    bark.reset();
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativePushAudio(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    frames: jshortArray,
) {
    if handle == 0 {
        throw_state(&mut env, "BarkPipeline not initialized");
        return;
    }

    // SAFETY: frames is a valid jshortArray provided by the JNI runtime.
    let jarray = unsafe { JShortArray::from_raw(frames) };
    let len = match env.get_array_length(&jarray) {
        Ok(n) => n as usize,
        Err(e) => {
            throw_arg(&mut env, &format!("Failed to get array length: {e}"));
            return;
        }
    };

    if len == 0 {
        return;
    }

    let mut buf = vec![0i16; len];
    if let Err(e) = env.get_short_array_region(&jarray, 0, &mut buf) {
        throw_arg(&mut env, &format!("Failed to read audio frames: {e}"));
        return;
    }

    // SAFETY: see deref_handle's safety contract — the mutable reference lives
    // only for the duration of this call and cannot alias.
    let bark = unsafe { deref_handle(handle) };
    if let Err(e) = bark.push_audio(&buf) {
        throw_state(&mut env, &format!("Push audio failed: {e}"));
    }
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeFinalize(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    if handle == 0 {
        throw_state(&mut env, "BarkPipeline not initialized");
        return std::ptr::null_mut();
    }

    // SAFETY: see deref_handle's safety contract — exclusive &mut for this
    // call, no aliasing possible through JNI.
    let bark = unsafe { deref_handle(handle) };

    let text = match bark.finalize() {
        Ok(t) => t,
        Err(e) => {
            throw_state(&mut env, &format!("Finalize failed: {e}"));
            return std::ptr::null_mut();
        }
    };

    match env.new_string(&text) {
        Ok(js) => js.into_raw(),
        Err(_) => {
            throw_state(&mut env, "Failed to create Java string from result");
            std::ptr::null_mut()
        }
    }
}
