use std::env;
use std::ffi::{c_void, CString};
use std::os::raw::c_char;
use std::path::PathBuf;
use std::ptr::{null_mut, eq};
use std::thread;

use dlopen::symbor::Library;
use jni_sys::{JavaVM, JavaVMInitArgs, JavaVMOption, jclass, jint, jmethodID, JNI_FALSE, JNI_VERSION_1_8, JNIEnv, jobject, jobjectArray, jvalue, JavaVMAttachArgs};
use log::*;

use crate::errors::*;
use crate::descriptor::JvmParameters;
use crate::UserInterface;

pub struct JvmStarter {}

impl JvmStarter {
    pub fn start_jvm(descriptor: &JvmParameters, installation_root: &PathBuf, ui: &UserInterface) -> Result<()> {
        unsafe {
            let mut opts = Vec::with_capacity(descriptor.options.len());
            for option in descriptor.options.iter() {
                debug!("adding option {}", option);

                let jvm_opt = JavaVMOption {
                    optionString: c_str(option.as_str()),
                    extraInfo: null_mut(),
                };
                opts.push(jvm_opt);
            }

            let vm_args = JavaVMInitArgs {
                ignoreUnrecognized: JNI_FALSE,
                version: JNI_VERSION_1_8,
                options: opts.as_ptr() as _,
                nOptions: opts.len() as _,
            };

            // set PATH to the location of the native libraries needed by the JVM
            let jvm_path = installation_root.join(&descriptor.jvm_path);
            env::set_var("PATH", &jvm_path);

            let lib = Library::open(jvm_path.join(&descriptor.jvm_library)).expect("failed to load JVM library");

            // change to installation root (JAR locations are specified relative to this)
            debug!("Switching to {:?}", installation_root);
            env::set_current_dir(&installation_root)
                .chain_err(|| ErrorKind::JavaExecutionError(format!("Could not change to installation directory {:?}", &installation_root)))?;

            type CreateJavaVMFunction = unsafe extern fn(pvm: *mut *mut JavaVM, penv: *mut *mut c_void, args: *mut c_void) -> jint;
            let create_java_vm = lib
                .symbol::<CreateJavaVMFunction>("JNI_CreateJavaVM")
                .chain_err(|| ErrorKind::JavaExecutionError(format!("failed to load 'JNI_CreateJavaVM' from JVM library")))?;

            let mut ptr: *mut JavaVM = null_mut();
            let mut jvm_env: *mut JNIEnv = null_mut();
            create_java_vm(
                &mut ptr as *mut _,
                &mut jvm_env as *mut *mut JNIEnv as *mut *mut c_void,
                &vm_args as *const _ as _,
            );

            let method_arguments = JvmStarter::build_arguments(jvm_env);

            let class: jclass = (**jvm_env).FindClass.unwrap()(jvm_env as _, c_str(descriptor.main_class.as_str()));

            let method_id: jmethodID = (**jvm_env).GetStaticMethodID.unwrap()(jvm_env as _, class, c_str("main"), c_str("([Ljava/lang/String;)V"));

            let mut arguments = Vec::new();
            arguments.push(method_arguments);

            let vm_for_thread = ptr as usize;
            let main_class = descriptor.main_class.clone();
            let ui_clone = ui.clone();
            thread::spawn(move || {
                let vm = vm_for_thread as *mut JavaVM ;
                let mut jvm_env: *mut JNIEnv = null_mut();
                let thr_args = JavaVMAttachArgs {
                    version: JNI_VERSION_1_8,
                    name: c_str("await UI"),
                    group: null_mut(),
                };
                (**vm).AttachCurrentThread.unwrap()(
                    vm as *mut JavaVM,
                    &mut jvm_env as *mut *mut JNIEnv as *mut *mut c_void,
                    &thr_args as *const _ as _,
                );
                let class: jclass = (**jvm_env).FindClass.unwrap()(jvm_env as _, c_str(main_class.as_str()));
                let method_id: jmethodID = (**jvm_env).GetStaticMethodID.unwrap()(jvm_env as _, class, c_str("awaitUI"), c_str("()V"));
                if !eq(method_id, null_mut()) {
                    debug!("awaitUI() found in Java application. Calling it to determine when to hide splash screen");
                    (**jvm_env).CallStaticVoidMethodA.unwrap()(jvm_env as _, class, method_id, Vec::new().as_ptr());
                } else {
                    debug!("awaitUI() not found in Java application. Hide splash screen immediately");
                }
                (**vm).DetachCurrentThread.unwrap()(*vm as *mut JavaVM);
                ui_clone.application_visible();
            });

            (**jvm_env).CallStaticVoidMethodA.unwrap()(jvm_env as _, class, method_id, arguments.as_ptr());
        }

        ui.application_terminated();
        return Ok(());
    }

    unsafe fn build_arguments<'a>(jvm_env: *mut jni_sys::JNIEnv) -> jni_sys::jvalue {
        // find String class
        let class: jclass = (**jvm_env).FindClass.unwrap()(jvm_env as _, c_str("java/lang/String"));

        let args: Vec<String> = env::args().collect();

        // create new java string array instance with the same length as the arguments vector
        let application_arguments: jobjectArray = (**jvm_env).NewObjectArray.unwrap()(jvm_env as _, args.len() as i32, class, null_mut());

        for i in 0..args.len() {

            // get argument from vector
            let argument: &String = &args[i];

            // create new java string object
            let argument: jobject = (**jvm_env).NewStringUTF.unwrap()(jvm_env as _, c_str(argument.as_str()));

            // set object on array
            (**jvm_env).SetObjectArrayElement.unwrap()(jvm_env as _, application_arguments, i as _, argument);
        }

        return jvalue {
            l: application_arguments
        };
    }
}

fn c_str(string_value: &str) -> *mut c_char {
    return CString::new(string_value).unwrap().into_raw();
}