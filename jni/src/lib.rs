use jni::objects::{JObject, JShortArray, JString, JValue, ReleaseMode};
use jni::sys::{jint, jlong, jstring};
use jni::JNIEnv;

use bark_core::{Bark, BarkConfig};

const HANDLE_FIELD: &str = "nativeHandle";
const HANDLE_SIG: &str = "J";

fn box_handle(bark: Bark) -> jlong {
    Box::into_raw(Box::new(bark)) as jlong
}

unsafe fn deref_handle<'a>(handle: jlong) -> &'a mut Bark {
    &mut *(handle as *mut Bark)
}

unsafe fn drop_handle(handle: jlong) {
    let _ = Box::from_raw(handle as *mut Bark);
}

fn get_handle(env: &mut JNIEnv, this: &JObject) -> Option<jlong> {
    match env
        .get_field(this, HANDLE_FIELD, HANDLE_SIG)
        .and_then(|v| v.j())
    {
        Ok(handle) => Some(handle),
        Err(e) => {
            throw_state(
                env,
                &format!("Failed to read BarkPipeline native handle: {e}"),
            );
            None
        }
    }
}

fn set_handle(env: &mut JNIEnv, this: &JObject, handle: jlong) -> bool {
    match env.set_field(this, HANDLE_FIELD, HANDLE_SIG, JValue::Long(handle)) {
        Ok(()) => true,
        Err(e) => {
            throw_state(
                env,
                &format!("Failed to store BarkPipeline native handle: {e}"),
            );
            false
        }
    }
}

fn get_live_handle(env: &mut JNIEnv, this: &JObject) -> Option<jlong> {
    let handle = get_handle(env, this)?;
    if handle == 0 {
        throw_state(env, "BarkPipeline not initialized");
        None
    } else {
        Some(handle)
    }
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
    this: JObject,
    config_json: JString,
) {
    match get_handle(&mut env, &this) {
        Some(0) => {}
        Some(_) => {
            throw_state(&mut env, "BarkPipeline already initialized");
            return;
        }
        None => return,
    }

    let json: String = match env.get_string(&config_json) {
        Ok(s) => s.into(),
        Err(_) => {
            throw_arg(&mut env, "Failed to read config JSON string");
            return;
        }
    };

    let config: BarkConfig = match serde_json::from_str(&json) {
        Ok(c) => c,
        Err(e) => {
            throw_arg(&mut env, &format!("Invalid config JSON: {e}"));
            return;
        }
    };

    let bark = match Bark::new(config) {
        Ok(b) => b,
        Err(e) => {
            throw_state(&mut env, &format!("BarkPipeline init failed: {e}"));
            return;
        }
    };

    set_handle(&mut env, &this, box_handle(bark));
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeDestroy(
    mut env: JNIEnv,
    this: JObject,
) {
    let Some(handle) = get_handle(&mut env, &this) else {
        return;
    };

    if handle != 0 && set_handle(&mut env, &this, 0) {
        // SAFETY: handle was produced by box_handle and has just been
        // detached from the Java object, so this call owns it.
        unsafe { drop_handle(handle) }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeReset(
    mut env: JNIEnv,
    this: JObject,
) {
    let Some(handle) = get_live_handle(&mut env, &this) else {
        return;
    };

    // SAFETY: the Java object serializes access to its native handle.
    let bark = unsafe { deref_handle(handle) };
    bark.reset();
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativePushAudio(
    mut env: JNIEnv,
    this: JObject,
    data: JShortArray,
    samples: jint,
) {
    let Some(handle) = get_live_handle(&mut env, &this) else {
        return;
    };

    if samples < 0 {
        throw_arg(&mut env, "Sample count must be non-negative");
        return;
    }

    let len = match env.get_array_length(&data) {
        Ok(n) => n as usize,
        Err(e) => {
            throw_arg(&mut env, &format!("Failed to get array length: {e}"));
            return;
        }
    };

    let samples = samples as usize;
    if samples > len {
        throw_arg(&mut env, "Sample count exceeds array length");
        return;
    }
    if samples == 0 {
        return;
    }

    let result = {
        // SAFETY: this call creates the only native elements view for this Java
        // array during this JNI method, and the slice is not kept after return.
        let elements = match unsafe { env.get_array_elements(&data, ReleaseMode::NoCopyBack) } {
            Ok(elements) => elements,
            Err(e) => {
                throw_arg(&mut env, &format!("Failed to read audio data: {e}"));
                return;
            }
        };

        // SAFETY: the Java object serializes access to its native handle.
        let bark = unsafe { deref_handle(handle) };
        bark.push_audio(&elements[..samples])
    };

    if let Err(e) = result {
        throw_state(&mut env, &format!("Push audio failed: {e}"));
    }
}

#[no_mangle]
pub extern "system" fn Java_com_mrsobakin_bark_BarkPipeline_nativeFinalize(
    mut env: JNIEnv,
    this: JObject,
) -> jstring {
    let Some(handle) = get_live_handle(&mut env, &this) else {
        return std::ptr::null_mut();
    };

    // SAFETY: the Java object serializes access to its native handle.
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
