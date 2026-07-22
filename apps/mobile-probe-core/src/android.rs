use std::ffi::c_void;
use std::ptr;
use std::sync::{Mutex, OnceLock};

use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue};
use jni::sys::jstring;
use jni::{JNIEnv, JavaVM};
use uc_engine::{HostCapabilityError, HostCapabilityErrorCategory, HostSecureStorage};

struct AndroidHost {
    vm: JavaVM,
    bridge: GlobalRef,
}

static HOST: OnceLock<Mutex<Option<AndroidHost>>> = OnceLock::new();
static ANDROID_CONTEXT: OnceLock<GlobalRef> = OnceLock::new();

pub(crate) struct AndroidSecureStorage;

impl HostSecureStorage for AndroidSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        with_host(|env, bridge| {
            let key = env.new_string(key).map_err(|_| unavailable_host_error())?;
            let key_object = JObject::from(key);
            let value = env
                .call_method(
                    bridge,
                    "secureGet",
                    "(Ljava/lang/String;)[B",
                    &[JValue::Object(&key_object)],
                )
                .map_err(|_| unavailable_host_error())?
                .l()
                .map_err(|_| unavailable_host_error())?;
            if value.is_null() {
                return Ok(None);
            }
            let bytes = env
                .convert_byte_array(JByteArray::from(value))
                .map_err(|_| unavailable_host_error())?;
            Ok(Some(bytes))
        })
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        with_host(|env, bridge| {
            let key = env.new_string(key).map_err(|_| unavailable_host_error())?;
            let bytes = env
                .byte_array_from_slice(value)
                .map_err(|_| unavailable_host_error())?;
            let key_object = JObject::from(key);
            let bytes_object = JObject::from(bytes);
            env.call_method(
                bridge,
                "secureSet",
                "(Ljava/lang/String;[B)V",
                &[JValue::Object(&key_object), JValue::Object(&bytes_object)],
            )
            .map_err(|_| unavailable_host_error())?;
            Ok(())
        })
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        with_host(|env, bridge| {
            let key = env.new_string(key).map_err(|_| unavailable_host_error())?;
            let key_object = JObject::from(key);
            env.call_method(
                bridge,
                "secureDelete",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&key_object)],
            )
            .map_err(|_| unavailable_host_error())?;
            Ok(())
        })
    }
}

fn with_host<T>(
    operation: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> Result<T, HostCapabilityError>,
) -> Result<T, HostCapabilityError> {
    let host = HOST
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| unavailable_host_error())?;
    let host = host.as_ref().ok_or_else(unavailable_host_error)?;
    let mut env = host
        .vm
        .attach_current_thread()
        .map_err(|_| unavailable_host_error())?;
    operation(&mut env, host.bridge.as_obj())
}

fn unavailable_host_error() -> HostCapabilityError {
    HostCapabilityError::new(
        HostCapabilityErrorCategory::Unavailable,
        "android secure storage unavailable",
    )
}

#[no_mangle]
pub extern "system" fn Java_app_uniclipboard_engineprobe_ProbeBridge_nativeInstallHost(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    bridge: JObject<'_>,
    context: JObject<'_>,
) {
    let host = env.get_java_vm().and_then(|vm| {
        let bridge = env.new_global_ref(bridge)?;
        let context = env.new_global_ref(context)?;
        ANDROID_CONTEXT.get_or_init(|| {
            unsafe {
                ndk_context::initialize_android_context(
                    vm.get_java_vm_pointer().cast::<c_void>(),
                    context.as_obj().as_raw().cast::<c_void>(),
                );
            }
            context
        });
        Ok(AndroidHost { vm, bridge })
    });
    if let Ok(host) = host {
        if let Ok(mut current) = HOST.get_or_init(|| Mutex::new(None)).lock() {
            *current = Some(host);
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_app_uniclipboard_engineprobe_ProbeBridge_nativeCommand(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    command: JString<'_>,
) -> jstring {
    let command: String = match env.get_string(&command) {
        Ok(command) => command.into(),
        Err(_) => return ptr::null_mut(),
    };
    match env.new_string(crate::probe_command(&command)) {
        Ok(response) => response.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}
