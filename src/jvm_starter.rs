use log::*;
use std::env;
use std::path::PathBuf;
use std::ptr::{eq, null_mut};
use std::thread;
use std::time::Instant;
use crate::descriptor::JvmParameters;
use crate::errors::*;
use crate::UserInterface;
use jni_simple::*;

pub struct JvmStarter {}

impl JvmStarter {
    pub fn start_jvm(descriptor: &JvmParameters, installation_root: &PathBuf, ui: &UserInterface) -> Result<()> {
        unsafe {
            let start = Instant::now();
            // set PATH to the location of the native libraries needed by the JVM
            let jvm_path = installation_root.join(&descriptor.jvm_path);
            env::set_var("PATH", &jvm_path);

            load_jvm_from_library(jvm_path.join(&descriptor.jvm_library).to_str().unwrap())
                .expect("failed to load jvm");

            // change to installation root (JAR locations are specified relative to this)
            debug!("Switching to {:?}", installation_root);
            env::set_current_dir(&installation_root)
                .chain_err(|| ErrorKind::JavaExecutionError(format!("Could not change to installation directory {:?}", &installation_root)))?;

            let (jvm, env) = JNI_CreateJavaVM_with_string_args(JNI_VERSION_1_8, &descriptor.options, false).expect("failed to create jvm");

            let main_class = env.FindClass(descriptor.main_class.as_str());
            let main_method = env.GetStaticMethodID(main_class, "main", "([Ljava/lang/String;)V");

            let string_class = env.FindClass("java/lang/String");
            let args: Vec<String> = env::args().collect();
            let main_method_string_parameter_array = env.NewObjectArray(args.len() as i32, string_class, null_mut());
            for i in 0..args.len() {
                let argument = env.NewStringUTF(args[i].as_str());
                env.SetObjectArrayElement(main_method_string_parameter_array, i as i32, argument);
            }

            let ui_clone = ui.clone();
            let main_class_name = descriptor.main_class.clone();
            thread::spawn(move || {
                let jvm = JNI_GetCreatedJavaVMs_first().unwrap().unwrap();
                jvm.AttachCurrentThread_str(JNI_VERSION_1_8, "await UI", null_mut()).expect("Could not attach thread");
                let env = jvm.GetEnv::<jni_simple::JNIEnv>(JNI_VERSION_1_8).unwrap();
                let main_class = env.FindClass(main_class_name.as_str());
                let await_ui_method = env.GetStaticMethodID(main_class, "awaitUI", "()V");
                if !eq(await_ui_method, null_mut()) {
                    debug!("awaitUI() found in Java application. Calling it to determine when to hide splash screen");
                    env.CallStaticVoidMethod0(main_class, await_ui_method);
                } else {
                    debug!("awaitUI() not found in Java application. Hide splash screen immediately");
                }
                let _ = jvm.DetachCurrentThread();
                ui_clone.application_visible();
            });

            let elapsed = start.elapsed();
            info!("Starting JVM took {} ms", elapsed.as_millis());
            env.CallStaticVoidMethod1(main_class, main_method, main_method_string_parameter_array);

            jvm.DestroyJavaVM();
        }

        ui.application_terminated();
        return Ok(());
    }
}
