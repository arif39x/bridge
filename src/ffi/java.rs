#![cfg(feature = "java-interop")]
use crate::engine;
use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use once_cell::sync::Lazy;
use std::sync::Once;
use tokio::runtime::Runtime;

static RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime for Java"));

/// This example assumes a package: io.bridge.core.Bridge
#[no_mangle]
pub extern "system" fn Java_io_bridgeorm_core_Bridge_connectNative(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
) -> jstring {
    let url_str: String = env
        .get_string(&url)
        .expect("Invalid URL string from Java")
        .into();

    let result = RUNTIME.block_on(async { engine::db::connect(&url_str, None).await });

    match result {
        Ok(_) => {
            let output = env
                .new_string("SUCCESS")
                .expect("Couldn't create java string!");
            output.into_raw()
        }
        Err(e) => {
            let err_msg = format!("ERROR: {}", e);
            let output = env
                .new_string(err_msg)
                .expect("Couldn't create java string!");
            output.into_raw()
        }
    }
}

fn to_java_string(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s)
        .expect("Failed to create Java string")
        .into_raw()
}
