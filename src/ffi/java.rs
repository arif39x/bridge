#![cfg(feature = "java-interop")]
use crate::engine;
use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use once_cell::sync::Lazy;
use std::sync::Once;
use tokio::runtime::Runtime;

fn get_runtime() -> Result<&'static Runtime, String> {
    static RUNTIME: Lazy<Result<Runtime, String>> = Lazy::new(|| {
        Runtime::new().map_err(|e| format!("Failed to create Tokio runtime for Java: {}", e))
    });
    RUNTIME.as_ref().map_err(|e| e.clone())
}

/// This example assumes a package: io.bridge.core.Bridge
#[no_mangle]
pub extern "system" fn Java_io_bridgeorm_core_Bridge_connectNative(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
) -> jstring {
    let url_str: String = match env.get_string(&url) {
        Ok(s) => s.into(),
        Err(_) => {
            return to_java_string(&mut env, "ERROR: Invalid URL string from Java");
        }
    };

    let runtime = match get_runtime() {
        Ok(rt) => rt,
        Err(e) => {
            return to_java_string(&mut env, &format!("ERROR: {}", e));
        }
    };

    let result = runtime.block_on(async { engine::db::connect(&url_str, None).await });

    match result {
        Ok(_) => to_java_string(&mut env, "SUCCESS"),
        Err(e) => to_java_string(&mut env, &format!("ERROR: {}", e)),
    }
}

fn to_java_string(env: &mut JNIEnv, s: &str) -> jstring {
    match env.new_string(s) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
