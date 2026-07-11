//! Login item management via macOS SMAppService (macOS 13+).

use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoLaunchStatus {
    Enabled,
    Disabled,
    RequiresApproval,
    Unsupported,
}

pub fn is_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_version_at_least(13, 0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

pub fn status() -> AutoLaunchStatus {
    #[cfg(target_os = "macos")]
    {
        if !is_supported() {
            return AutoLaunchStatus::Unsupported;
        }

        let service = main_app_service();
        match unsafe { service.status() } {
            objc2_service_management::SMAppServiceStatus::Enabled => AutoLaunchStatus::Enabled,
            objc2_service_management::SMAppServiceStatus::RequiresApproval => {
                AutoLaunchStatus::RequiresApproval
            }
            objc2_service_management::SMAppServiceStatus::NotRegistered => {
                AutoLaunchStatus::Disabled
            }
            _ => AutoLaunchStatus::Disabled,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        AutoLaunchStatus::Unsupported
    }
}

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        if !is_supported() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "SMAppService requires macOS 13 or later",
            ));
        }

        let service = main_app_service();
        if enabled {
            match unsafe { service.registerAndReturnError() } {
                Ok(()) => Ok(()),
                Err(error) => {
                    let io_error = ns_error_to_io(error);
                    if io_error.kind() == io::ErrorKind::AlreadyExists {
                        Ok(())
                    } else {
                        Err(io_error)
                    }
                }
            }
        } else {
            match unsafe { service.unregisterAndReturnError() } {
                Ok(()) => Ok(()),
                Err(error) => {
                    let io_error = ns_error_to_io(error);
                    if io_error.kind() == io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(io_error)
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = enabled;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "auto launch is only supported on macOS",
        ))
    }
}

pub fn open_login_items_settings() {
    #[cfg(target_os = "macos")]
    {
        if is_supported() {
            unsafe {
                objc2_service_management::SMAppService::openSystemSettingsLoginItems();
            }
        }
    }
}

/// Align SMAppService registration with the persisted settings value.
pub fn sync_from_settings() {
    let settings = fly_settings::Settings::load().unwrap_or_default();
    let current = status();

    let needs_register = settings.auto_launch
        && !matches!(current, AutoLaunchStatus::Enabled | AutoLaunchStatus::RequiresApproval);
    let needs_unregister = !settings.auto_launch && current == AutoLaunchStatus::Enabled;

    if needs_register {
        if let Err(e) = set_enabled(true) {
            log::warn!("Failed to register login item: {e}");
        }
    } else if needs_unregister {
        if let Err(e) = set_enabled(false) {
            log::warn!("Failed to unregister login item: {e}");
        }
    }
}

#[cfg(target_os = "macos")]
fn main_app_service() -> objc2::rc::Retained<objc2_service_management::SMAppService> {
    unsafe { objc2_service_management::SMAppService::mainAppService() }
}

#[cfg(target_os = "macos")]
fn ns_error_to_io(error: objc2::rc::Retained<objc2_foundation::NSError>) -> io::Error {
    let description = error.localizedDescription().to_string();

    let code = error.code();
    let kind = match code {
        1 => io::ErrorKind::AlreadyExists,   // kSMErrorAlreadyRegistered
        2 => io::ErrorKind::NotFound,         // kSMErrorJobNotFound
        3 => io::ErrorKind::PermissionDenied, // kSMErrorLaunchDeniedByUser
        _ => io::ErrorKind::Other,
    };

    io::Error::new(kind, description)
}

#[cfg(target_os = "macos")]
fn macos_version_at_least(major: i32, minor: i32) -> bool {
    let version = macos_product_version();
    version
        .map(|(release_major, release_minor)| {
            release_major > major || (release_major == major && release_minor >= minor)
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn macos_product_version() -> Option<(i32, i32)> {
    use std::ffi::CStr;

    let mut size: usize = 0;
    let name = c"kern.osproductversion";
    unsafe {
        if libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }

        let mut buf = vec![0u8; size];
        if libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr() as *mut _,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }

        let version = CStr::from_ptr(buf.as_ptr() as *const _).to_string_lossy();
        parse_product_version(&version)
    }
}

#[cfg(target_os = "macos")]
fn parse_product_version(version: &str) -> Option<(i32, i32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}
