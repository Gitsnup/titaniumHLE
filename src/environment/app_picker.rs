//! App picker GUI.
//!
//! This also includes a license text viewer. The license text viewer is needed
//! on Android, where the command-line way to view license text doesn't exist.

use crate::bundle::Bundle;
use crate::frameworks::core_graphics::cg_bitmap_context::{
    CGBitmapContextCreate, CGBitmapContextCreateImage,
};
use crate::frameworks::core_graphics::cg_color_space::CGColorSpaceCreateDeviceRGB;
use crate::frameworks::core_graphics::cg_context::{
    CGContextFillRect, CGContextRelease, CGContextScaleCTM, CGContextSetRGBFillColor,
    CGContextTranslateCTM,
};
use crate::frameworks::core_graphics::cg_image::{self, kCGImageAlphaPremultipliedLast};
use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_run_loop::run_run_loop_single_iteration;
use crate::frameworks::foundation::ns_string;
use crate::frameworks::uikit::ui_font::{
    UITextAlignmentCenter, UITextAlignmentLeft, UITextAlignmentRight,
};
use crate::frameworks::uikit::ui_graphics::{UIGraphicsPopContext, UIGraphicsPushContext};
use crate::frameworks::uikit::ui_view::ui_control::ui_button::{
    UIButtonTypeCustom, UIButtonTypeRoundedRect,
};
use crate::frameworks::uikit::ui_view::ui_control::{
    UIControlEventTouchUpInside, UIControlEventValueChanged, UIControlStateNormal,
};
use crate::fs::BundleData;
use crate::image::Image;
use crate::mem::Ptr;
use crate::objc::{id, msg, msg_class, nil, objc_classes, release, ClassExports, HostObject};
use crate::options::Options;
use crate::paths;
use crate::window::DeviceOrientation;
use crate::Environment;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

struct AppInfo {
    path: PathBuf,
    display_name: String,
    architecture: AppArchitecture,
    icon: Option<Image>,
    /// `NSString*`
    display_name_ns_string: Option<id>,
    /// `UIImage*`
    icon_ui_image: Option<id>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppArchitecture {
    Arm32,
    Arm64,
}

pub fn app_picker(options: Options) -> Result<(PathBuf, Vec<String>), String> {
    let apps_dir = paths::user_data_base_path().join(paths::APPS_DIR);

    let apps: Result<Vec<AppInfo>, String> = if !apps_dir.is_dir() {
        Err(format!("The {} directory couldn't be found. Check you're running touchHLE from the right directory.", apps_dir.display()))
    } else {
        enumerate_apps(&apps_dir)
            .map_err(|err| {
                format!(
                    "Couldn't get list of apps in the {} directory: {}.",
                    apps_dir.display(),
                    err
                )
            })
            .and_then(|apps| {
                if apps.is_empty() {
                    Err(format!(
                        "No apps were found in the {} directory.",
                        apps_dir.display()
                    ))
                } else {
                    Ok(apps)
                }
            })
    };

    show_app_picker_gui(options, apps)
}

fn enumerate_apps(apps_dir: &Path) -> Result<Vec<AppInfo>, std::io::Error> {
    let mut apps = Vec::new();
    for app in std::fs::read_dir(apps_dir)? {
        let app_path = app?.path();
        if app_path.extension() != Some(OsStr::new("app"))
            && app_path.extension() != Some(OsStr::new("ipa"))
        {
            continue;
        }

        // TODO: avoid loading the whole FS somehow?
        let (bundle, fs) = match BundleData::open_any(&app_path).and_then(|bundle_data| {
            Bundle::new_bundle_and_fs_from_host_path(bundle_data, /* read_only_mode: */ true)
        }) {
            Ok(ok) => ok,
            Err(e) => {
                log!(
                    "Warning: couldn't open app bundle {}: {} (skipping)",
                    app_path.display(),
                    e
                );
                continue;
            }
        };

        // TODO: what if this crashes?
        let display_name = bundle.display_name().to_owned();
        let executable = fs
            .read(bundle.executable_path())
            .map_err(|e| std::io::Error::other(format!("couldn't read executable: {e:?}")))?;
        let architecture = if crate::arm64::detect_arm64_executable(&executable) {
            AppArchitecture::Arm64
        } else {
            AppArchitecture::Arm32
        };

        let icon = match bundle.load_icon(&fs) {
            Ok(icon) => Some(icon),
            Err(e) => {
                log!("Warning: couldn't load icon for app bundle {}: {} (displaying placeholder instead)", app_path.display(), e);
                None
            }
        };

        apps.push(AppInfo {
            path: app_path,
            display_name,
            architecture,
            icon,
            display_name_ns_string: None,
            icon_ui_image: None,
        });
    }

    apps.sort_by_key(|app| app.display_name.to_uppercase());

    Ok(apps)
}

#[derive(Default)]
struct AppPickerDelegateHostObject {
    icon_tapped: id,
    copyright_show: bool,
    copyright_hide: bool,
    copyright_prev: bool,
    copyright_next: bool,
    quick_options_show: bool,
    quick_options_hide: bool,
    scale_hack_default: bool,
    scale_hack1: bool,
    scale_hack2: bool,
    scale_hack3: bool,
    scale_hack4: bool,
    orientation_default: bool,
    orientation_portrait_upside_down: bool,
    orientation_landscape_left: bool,
    orientation_landscape_right: bool,
    tilt_sensitivity_default: bool,
    tilt_sensitivity_half: bool,
    tilt_sensitivity_three_quarters: bool,
    tilt_sensitivity_one_and_a_half: bool,
    tilt_sensitivity_double: bool,
    region_pulldown_toggle: bool,
    region_pulldown_close: bool,
    region_default: bool,
    region_us: bool,
    region_gb: bool,
    region_jp: bool,
    region_fr: bool,
    region_de: bool,
    analog_stick_tilt_controls: Option<bool>,
    network: Option<bool>,
    fullscreen: Option<bool>,
    print_fps: Option<bool>,
    force_composition: Option<bool>,
    ignore_gl_errors: Option<bool>,
    error_popups: Option<bool>,
    quick_options_prev_page: bool,
    quick_options_next_page: bool,
    arm32_apps: bool,
    arm64_apps: bool,
    /// Index into the selected app's settings toggles, or `usize::MAX` if none.
    /// The switch that was flipped, if any.
    setting_toggled: id,
    setting_toggled_value: bool,
}
impl HostObject for AppPickerDelegateHostObject {}

/// A toggle from an app's `Settings.bundle`, together with the app it
/// belongs to. Toggles from every installed app are shown in the Quick
/// options panel, since there's no Settings app to host them.
#[derive(Clone)]
struct AppToggle {
    app_path: PathBuf,
    app_name: String,
    toggle: crate::environment::settings_bundle::SettingsToggle,
}

/// The toggles declared by the installed apps' `Settings.bundle` files, plus
/// the switches that display them. Empty if no app ships a settings bundle.
#[derive(Default)]
struct AppSettingsStuff {
    toggles: Vec<AppToggle>,
    switches: Vec<id>,
}

impl AppSettingsStuff {
    /// Which toggle a switch belongs to, identified by object identity.
    fn index_of_switch(&self, switch: id) -> Option<usize> {
        self.switches.iter().position(|&s| s == switch)
    }
}

pub const DYLIB: crate::dyld::HostDylib = crate::dyld::HostDylib {
    // Not a real iOS dylib obviously. This shouldn't really be in the list of
    // dylibs if we can avoid it somehow (TODO?).
    path: "/.touchHLE/AppPickerHelpers.dylib",
    aliases: &[],
    class_exports: &[CLASSES],
    constant_exports: &[],
    function_exports: &[],
};

/// Be careful! These classes go in the normal class list, just like everything
/// else, so an app could try to instantiate them. Don't give them special
/// powers that could be exploited!
const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation _touchHLE_AppPickerDelegate: NSObject

- (())iconTapped:(id)sender {
    // There is no allocWithZone: that creates AppPickerDelegateHostObject, so
    // this downcast effectively acts as an assertion that this class is being
    // used within the app picker, so it can't be abused. :)
    let host_obj = env.objc.borrow_mut::<AppPickerDelegateHostObject>(this);
    host_obj.icon_tapped = sender;
}

- (())copyrightInfoShow {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).copyright_show = true;
}
- (())copyrightInfoHide {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).copyright_hide = true;
}
- (())copyrightInfoPrevPage {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).copyright_prev = true;
}
- (())copyrightInfoNextPage {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).copyright_next = true;
}

- (())quickOptionsShow {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).quick_options_show = true;
}
- (())quickOptionsHide {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).quick_options_hide = true;
}
- (())scaleHackDefault {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack_default = true;
}
- (())scaleHack1 {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack1 = true;
}
- (())scaleHack2 {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack2 = true;
}
- (())scaleHack3 {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack3 = true;
}
- (())scaleHack4 {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).scale_hack4 = true;
}
- (())orientationDefault {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).orientation_default = true;
}
- (())orientationPortraitUpsideDown {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).orientation_portrait_upside_down = true;
}
- (())orientationLandscapeLeft {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).orientation_landscape_left = true;
}
- (())orientationLandscapeRight {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).orientation_landscape_right = true;
}
- (())tiltSensitivityDefault {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).tilt_sensitivity_default = true;
}
- (())tiltSensitivityHalf {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).tilt_sensitivity_half = true;
}
- (())tiltSensitivityThreeQuarters {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this)
        .tilt_sensitivity_three_quarters = true;
}
- (())tiltSensitivityOneAndAHalf {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this)
        .tilt_sensitivity_one_and_a_half = true;
}
- (())tiltSensitivityDouble {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).tilt_sensitivity_double = true;
}
- (())regionPulldownToggle {
    env.objc
        .borrow_mut::<AppPickerDelegateHostObject>(this)
        .region_pulldown_toggle = true;
}
- (())regionPulldownClose {
    env.objc
        .borrow_mut::<AppPickerDelegateHostObject>(this)
        .region_pulldown_close = true;
}
- (())regionDefault {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).region_default = true;
}
- (())regionUS {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).region_us = true;
}
- (())regionGB {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).region_gb = true;
}
- (())regionJP {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).region_jp = true;
}
- (())regionFR {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).region_fr = true;
}
- (())regionDE {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).region_de = true;
}
- (())analogStickTiltControls:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).analog_stick_tilt_controls = Some(switch_state);
}
- (())network:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).network = Some(switch_state);
}
- (())fullscreen:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).fullscreen = Some(switch_state);
}
- (())printFps:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).print_fps = Some(switch_state);
}
- (())forceComposition:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).force_composition = Some(switch_state);
}
- (())ignoreGlErrors:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).ignore_gl_errors = Some(switch_state);
}
- (())errorPopups:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).error_popups = Some(switch_state);
}
- (())settingToggled:(id)switch { // UISwitch*
    let switch_state: bool = msg![env; switch isOn];
    let host_obj = env.objc.borrow_mut::<AppPickerDelegateHostObject>(this);
    host_obj.setting_toggled = switch;
    host_obj.setting_toggled_value = switch_state;
}
- (())quickOptionsPrevPage {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).quick_options_prev_page = true;
}
- (())quickOptionsNextPage {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).quick_options_next_page = true;
}
- (())arm32Apps {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).arm32_apps = true;
}
- (())arm64Apps {
    env.objc.borrow_mut::<AppPickerDelegateHostObject>(this).arm64_apps = true;
}

- (())addGame {
    // Assert (see above).
    let _ = env.objc.borrow_mut::<AppPickerDelegateHostObject>(this);

    // On Android, opening the user data directory in a file manager is no
    // longer a reliable way to add games, so we use the in-app document
    // picker flow instead. Elsewhere, opening the directory in the system
    // file manager is still the most convenient option.
    let url = if std::env::consts::OS == "android" {
        paths::url_for_adding_game()
    } else {
        paths::url_for_opening_user_data_dir()
    };
    match url {
        Ok(url) => {
            // Our `openURL:` implementation is bypassed because it doesn't
            // allow non-web URLs.
            let url_res = crate::window::open_url(env, &url);
            if let Err(e) = url_res {
                echo!("Couldn't open URL {:?}: {}", url, e);
            } else {
                // Exiting is deliberate, on all platforms: the app picker
                // only scans for games at startup, so the importer runs
                // without touchHLE running at all. On Android this also
                // keeps the emulator from running while its GL surface is
                // torn down for the system file picker, which it doesn't
                // handle gracefully.
                echo!("Opened {:?}, exiting.", url);
                std::process::exit(0);
            }
        },
        Err(e) => echo!("Couldn't add game: {}", e),
    }
}

- (())visitWebsite {
    // Assert (see above).
    let _ = env.objc.borrow_mut::<AppPickerDelegateHostObject>(this);

    let url = ns_string::get_static_str(env, "https://touchhle.org/");
    let url: id = msg_class![env; NSURL URLWithString:url];
    let ui_application: id = msg_class![env; UIApplication sharedApplication];
    assert!(msg![env; ui_application openURL:url]);
}

@end

};

fn show_app_picker_gui(
    options: Options,
    apps: Result<Vec<AppInfo>, String>,
) -> Result<(PathBuf, Vec<String>), String> {
    let icon = {
        let bytes: &[u8] = match crate::branding() {
            "" => include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/icon.png")),
            "UNOFFICIAL" => {
                include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/icon_preview.png"))
            }
            "PREVIEW" => {
                include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/icon_preview.png"))
            }
            _ => panic!(),
        };
        let mut image = Image::from_bytes(bytes).unwrap();
        // should match Bundle::load_icon()
        image.round_corners(
            (10.0 / 57.0) * (image.dimensions().0 as f32),
            /* four_corners: */ true,
            /* add_sheen: */ true,
        );
        image
    };
    let environment = Environment::new_without_app(options, icon)?;
    Ok(environment.run_app_picker(|env| app_picker_inner(env, apps)))
}

fn app_picker_inner(
    env: &mut Environment,
    mut apps: Result<Vec<AppInfo>, String>,
) -> (PathBuf, Vec<String>) {
    let mut option_args = Vec::new();
    // Note that objects are generally not released in this code, because they
    // don't need to be: the entire Environment is thrown away at the end.

    // Bypassing UIApplicationMain!
    let ui_application: id = msg_class![env; UIApplication new];
    let delegate = env
        .objc
        .get_known_class("_touchHLE_AppPickerDelegate", &mut env.mem);
    let delegate = env.objc.alloc_object(
        delegate,
        Box::<AppPickerDelegateHostObject>::default(),
        &mut env.mem,
    );
    () = msg![env; ui_application setDelegate:delegate];

    let screen: id = msg_class![env; UIScreen mainScreen];
    let bounds: CGRect = msg![env; screen bounds];

    let window: id = msg_class![env; UIWindow alloc];
    let window: id = msg![env; window initWithFrame:bounds];

    let app_frame: CGRect = msg![env; screen applicationFrame];
    let main_view: id = msg_class![env; UIView alloc];
    let main_view: id = msg![env; main_view initWithFrame:app_frame];
    () = msg![env; window addSubview:main_view];

    // Wallpaper
    let mut found_wallpaper = false;
    let mut have_wallpaper = false;
    for candidate in paths::WALLPAPER_FILES {
        let candidate = paths::user_data_base_path().join(candidate);
        if !candidate.exists() {
            continue;
        }
        found_wallpaper = true;

        let image = match std::fs::read(&candidate) {
            Ok(image) => image,
            Err(e) => {
                log!("Warning: couldn't read {}: {}", candidate.display(), e);
                break;
            }
        };
        let image = match Image::from_bytes(&image) {
            Ok(image) => image,
            Err(e) => {
                log!("Warning: couldn't decode {}: {}", candidate.display(), e);
                break;
            }
        };

        let image = cg_image::from_image(env, image);
        let image: id = msg_class![env; UIImage imageWithCGImage:image];
        let wallpaper: id = msg_class![env; UIImageView alloc];
        let wallpaper: id = msg![env; wallpaper initWithImage:image];
        () = msg![env; wallpaper setFrame:(CGRect {
            origin: CGPoint {
                x: 0.0,
                y: 0.0,
            },
            size: app_frame.size,
        })];
        () = msg![env; wallpaper setAlpha:(0.5 as CGFloat)];
        () = msg![env; main_view addSubview:wallpaper];
        have_wallpaper = true;
        break;
    }
    if !found_wallpaper {
        let CGSize { width, height } = app_frame.size;
        log!(
            "No wallpaper found; filename can be one of: {}; ideal size is {}×{} pixels",
            paths::WALLPAPER_FILES.join(", "),
            width,
            height,
        );
    }

    // Version label
    {
        let label_frame = CGRect {
            origin: CGPoint {
                x: 0.0,
                y: app_frame.size.height - 20.0,
            },
            size: CGSize {
                width: app_frame.size.width - 5.0,
                height: 15.0,
            },
        };
        let label: id = msg_class![env; UILabel alloc];
        let label: id = msg![env; label initWithFrame:label_frame];
        let text = ns_string::from_rust_string(
            env,
            format!(
                "titaniumHLE {}{}{}",
                crate::branding(),
                if crate::branding().is_empty() {
                    ""
                } else {
                    " "
                },
                crate::VERSION
            ),
        );
        () = msg![env; label setText:text];
        () = msg![env; label setTextAlignment:UITextAlignmentRight];
        let font_size: CGFloat = 12.0;
        let font: id = msg_class![env; UIFont systemFontOfSize:font_size];
        () = msg![env; label setFont:font];
        let text_color: id = if have_wallpaper {
            msg_class![env; UIColor whiteColor]
        } else {
            msg_class![env; UIColor lightGrayColor]
        };
        () = msg![env; label setTextColor:text_color];
        let bg_color: id = msg_class![env; UIColor clearColor];
        () = msg![env; label setBackgroundColor:bg_color];
        () = msg![env; main_view addSubview:label];
    }

    let brand_color: id = if crate::branding() == "UNOFFICIAL" {
        msg_class![env; UIColor redColor]
    } else {
        msg_class![env; UIColor grayColor]
    };

    for i in 1..=7 {
        let label_frame = CGRect {
            origin: CGPoint {
                x: 0.0,
                y: (app_frame.size.height / 8.0) * (i as f32) - 25.0,
            },
            size: CGSize {
                width: app_frame.size.width,
                height: 50.0,
            },
        };
        let label: id = msg_class![env; UILabel alloc];
        let label: id = msg![env; label initWithFrame:label_frame];
        let text = ns_string::from_rust_string(env, crate::branding().to_owned());
        () = msg![env; label setText:text];
        () = msg![env; label setTextAlignment:(if i % 2 == 0 { UITextAlignmentLeft } else { UITextAlignmentRight })];
        let font_size: CGFloat = 48.0;
        let font: id = msg_class![env; UIFont systemFontOfSize:font_size];
        () = msg![env; label setFont:font];
        () = msg![env; label setTextColor:brand_color];
        let bg_color: id = msg_class![env; UIColor clearColor];
        () = msg![env; label setBackgroundColor:bg_color];
        () = msg![env; main_view addSubview:label];
    }

    let divider = app_frame.size.height - 100.0;

    let mut selected_architecture = AppArchitecture::Arm32;
    let mut selected_app_indices = match &apps {
        Ok(apps) => app_indices_for_architecture(apps, selected_architecture),
        Err(_) => Vec::new(),
    };
    let mut icon_grid_stuff = match &mut apps {
        Ok(ref mut apps) => {
            let mut icon_grid_stuff = make_icon_grid(
                env,
                delegate,
                main_view,
                app_frame,
                selected_app_indices.len(),
                have_wallpaper,
            );
            update_icon_grid(env, &mut icon_grid_stuff, apps, &selected_app_indices, 0);
            Some(icon_grid_stuff)
        }
        Err(e) => {
            let label_frame = CGRect {
                origin: CGPoint { x: 10.0, y: 10.0 },
                size: CGSize {
                    width: app_frame.size.width - 20.0,
                    height: divider - 20.0,
                },
            };
            let label: id = msg_class![env; UILabel alloc];
            let label: id = msg![env; label initWithFrame:label_frame];
            let text = ns_string::from_rust_string(env, e.clone());
            () = msg![env; label setText:text];
            () = msg![env; label setTextAlignment:UITextAlignmentCenter];
            () = msg![env; label setNumberOfLines:0]; // unlimited
            let text_color: id = msg_class![env; UIColor lightGrayColor];
            () = msg![env; label setTextColor:text_color];
            let bg_color: id = msg_class![env; UIColor clearColor];
            () = msg![env; label setBackgroundColor:bg_color];
            () = msg![env; main_view addSubview:label];
            None
        }
    };

    let buttons_row_center = divider + (app_frame.size.height - divider) / 4.0;
    let buttons_row2_center = divider + (app_frame.size.height - divider) / 1.6;
    make_button_row(
        env,
        delegate,
        main_view,
        app_frame.size,
        buttons_row_center,
        &[("Add game", "addGame"), ("Settings", "quickOptionsShow")],
        None,
    );
    make_button_row(
        env,
        delegate,
        main_view,
        app_frame.size,
        buttons_row2_center,
        &[
            ("Copyright info", "copyrightInfoShow"),
            ("touchHLE.org", "visitWebsite"),
        ],
        None,
    );

    let copyright_info_text = crate::licenses::get_text();
    let mut copyright_info_stuff = setup_copyright_info(env, delegate, main_view, app_frame);
    let mut copyright_info_page_idx = 0;

    // Built lazily on first open of the Settings panel. It shows every
    // installed app's Settings.bundle toggles, so it never needs rebuilding.
    let mut quick_options_stuff: Option<QuickOptionsStuff> = None;
    let mut quick_options_scale_hack: Option<NonZeroU32> = None;
    let mut quick_options_fullscreen: Option<()> = None;
    let mut quick_options_orientation: Option<DeviceOrientation> = None;
    let mut quick_options_tilt_sensitivity: Option<f32> = None;
    let mut quick_options_country_code: Option<&'static str> = None;
    let mut quick_options_analog_stick_tilt_controls = true;
    let mut quick_options_network = false;
    let mut quick_options_print_fps = false;
    let mut quick_options_force_composition = false;
    let mut quick_options_ignore_gl_errors = false;
    let mut quick_options_error_popups = true;
    let mut quick_options_page = 0usize;

    fn update_quick_option_buttons(env: &mut Environment, buttons: &[id], selected_idx: usize) {
        for (idx, &button) in buttons.iter().enumerate() {
            let background = make_frutiger_button_image(env, idx == selected_idx);
            () = msg![env; button setBackgroundImage:background
                                          forState:UIControlStateNormal];
            // Note: make_frutiger_button_image returns an autoreleased UIImage
            // and setBackgroundImage:forState: retains it, so it must NOT be
            // released here.
            // White text for readability on the darker bottom half
            let text_color: id = msg_class![env; UIColor whiteColor];
            () = msg![env; button setTitleColor:text_color
                                     forState:UIControlStateNormal];
        }
    }
    fn update_scale_hack_buttons(env: &mut Environment, buttons: &[id], value: Option<NonZeroU32>) {
        update_quick_option_buttons(env, buttons, value.map_or(0, |v| v.get() as usize));
    }
    fn update_orientation_buttons(
        env: &mut Environment,
        buttons: &[id],
        value: Option<DeviceOrientation>,
    ) {
        update_quick_option_buttons(
            env,
            buttons,
            value.map_or(0, |v| match v {
                DeviceOrientation::LandscapeLeft => 1,
                DeviceOrientation::LandscapeRight => 2,
                DeviceOrientation::PortraitUpsideDown => 3,
                _ => panic!(),
            }),
        );
    }
    fn update_tilt_sensitivity_buttons(env: &mut Environment, buttons: &[id], value: Option<f32>) {
        let selected_idx = match value {
            None => 0,
            Some(0.5) => 1,
            Some(0.75) => 2,
            Some(1.5) => 3,
            Some(2.0) => 4,
            _ => 0,
        };
        update_quick_option_buttons(env, buttons, selected_idx);
    }
    fn update_region_buttons(
        env: &mut Environment,
        pulldown: &RegionPulldownStuff,
        value: Option<&str>,
    ) {
        const REGION_CODES: [&str; 5] = ["US", "GB", "JP", "FR", "DE"];
        const ITEM_TITLES: [&str; 6] = ["Default", "US", "GB", "JP", "FR", "DE"];
        let selected_idx = value
            .and_then(|v| REGION_CODES.iter().position(|&code| code == v))
            .map_or(0, |idx| idx + 1);
        let title = format!("Region: {} \u{25BC}", ITEM_TITLES[selected_idx]);
        let text = ns_string::from_rust_string(env, title);
        let trigger = pulldown.trigger;
        () = msg![env; trigger setTitle:text forState:UIControlStateNormal];
        let background = make_frutiger_button_image(env, false);
        () = msg![env; trigger setBackgroundImage:background
                                       forState:UIControlStateNormal];
        for (idx, &button) in pulldown.item_buttons.iter().enumerate() {
            let item_title = if idx == selected_idx {
                format!("\u{25CF} {}", ITEM_TITLES[idx])
            } else {
                ITEM_TITLES[idx].to_string()
            };
            let text = ns_string::from_rust_string(env, item_title);
            () = msg![env; button setTitle:text forState:UIControlStateNormal];
            let color = if idx == selected_idx {
                // Vista glass blue for the picked region.
                let color: id = msg_class![env; UIColor colorWithRed:(0.06 as CGFloat)
                                                                green:(0.43 as CGFloat)
                                                                 blue:(0.79 as CGFloat)
                                                                alpha:(1.0 as CGFloat)];
                color
            } else {
                let color: id = msg_class![env; UIColor colorWithRed:(0.10 as CGFloat)
                                                                green:(0.17 as CGFloat)
                                                                 blue:(0.24 as CGFloat)
                                                                alpha:(1.0 as CGFloat)];
                color
            };
            () = msg![env; button setTitleColor:color forState:UIControlStateNormal];
        }
        set_region_pulldown_visible(env, pulldown, false);
    }
    if let Some(stuff) = &quick_options_stuff {
        update_scale_hack_buttons(env, &stuff.scale_hack_buttons, quick_options_scale_hack);
        update_orientation_buttons(env, &stuff.orientation_buttons, quick_options_orientation);
        update_region_buttons(env, &stuff.region_pulldown, quick_options_country_code);
        update_tilt_sensitivity_buttons(
            env,
            &stuff.tilt_sensitivity_buttons,
            quick_options_tilt_sensitivity,
        );
    }

    () = msg![env; window makeKeyAndVisible];

    let main_run_loop: id = msg_class![env; NSRunLoop mainRunLoop];
    // If an app is picked, this loop returns. If the user quits touchHLE, the
    // process exits.
    let app_path = loop {
        run_run_loop_single_iteration(env, main_run_loop);
        let host_obj = env.objc.borrow_mut::<AppPickerDelegateHostObject>(delegate);
        let icon_tapped = std::mem::take(&mut host_obj.icon_tapped);
        if icon_tapped != nil {
            match icon_grid_stuff.as_ref().unwrap().icon_map.get(&icon_tapped) {
                Some(&TappedIcon::App(app_idx)) => {
                    // Provide visual feedback that the app has been picked
                    // (it may take a while for the splash screen to appear etc)
                    () = msg![env; icon_tapped setAlpha:(0.5 as CGFloat)];
                    // Redraw screen, even if this makes the next frame early
                    // (the app picker will never be redrawn after this).
                    crate::frameworks::core_animation::recomposite_if_necessary(
                        env, /* force: */ true,
                    );
                    // Ensure touchHLE is responsive from the OS perspective,
                    // otherwise screen redraw might not show up? (Unclear if
                    // this explanation is correct.)
                    run_run_loop_single_iteration(env, main_run_loop);

                    let app_path = &apps.as_ref().unwrap()[app_idx].path;
                    echo!("Picked: {}", app_path.display());
                    break app_path.clone();
                }
                Some(&TappedIcon::ChangePage(page_idx)) => {
                    update_icon_grid(
                        env,
                        icon_grid_stuff.as_mut().unwrap(),
                        apps.as_mut().unwrap(),
                        &selected_app_indices,
                        page_idx,
                    );
                }
                None => (), // Tapped on a black space
            }
            continue;
        }
        if std::mem::take(&mut host_obj.copyright_show) {
            copyright_info_page_idx = 0;
            change_copyright_page(
                env,
                &mut copyright_info_stuff,
                &copyright_info_text,
                copyright_info_page_idx,
            );
            () = msg![env; (copyright_info_stuff.main_view) setHidden:false];
        } else if std::mem::take(&mut host_obj.copyright_hide) {
            () = msg![env; (copyright_info_stuff.main_view) setHidden:true];
        } else if std::mem::take(&mut host_obj.copyright_prev) && copyright_info_page_idx != 0 {
            copyright_info_page_idx -= 1;
            change_copyright_page(
                env,
                &mut copyright_info_stuff,
                &copyright_info_text,
                copyright_info_page_idx,
            );
        } else if std::mem::take(&mut host_obj.copyright_next)
            && Some(copyright_info_page_idx) != copyright_info_stuff.last_page_idx
        {
            copyright_info_page_idx += 1;
            change_copyright_page(
                env,
                &mut copyright_info_stuff,
                &copyright_info_text,
                copyright_info_page_idx,
            );
        } else if std::mem::take(&mut host_obj.quick_options_show) {
            // Built lazily on first open; never needs rebuilding, since it
            // describes every installed app rather than one selection.
            if quick_options_stuff.is_none() {
                let app_list = apps.as_ref().map(|apps| &apps[..]).unwrap_or(&[]);
                quick_options_stuff = Some(setup_quick_options(
                    env, delegate, main_view, app_frame, app_list,
                ));
                quick_options_page = 0;
                let stuff = quick_options_stuff.as_ref().unwrap();
                update_scale_hack_buttons(env, &stuff.scale_hack_buttons, quick_options_scale_hack);
                update_orientation_buttons(
                    env,
                    &stuff.orientation_buttons,
                    quick_options_orientation,
                );
                update_region_buttons(env, &stuff.region_pulldown, quick_options_country_code);
                update_tilt_sensitivity_buttons(
                    env,
                    &stuff.tilt_sensitivity_buttons,
                    quick_options_tilt_sensitivity,
                );
                update_quick_option_buttons(
                    env,
                    &stuff.architecture_buttons,
                    usize::from(selected_architecture == AppArchitecture::Arm64),
                );
            }
            let stuff = quick_options_stuff.as_ref().unwrap();
            () = msg![env; (stuff.main_view) setHidden:false];
        } else if std::mem::take(&mut host_obj.quick_options_hide) {
            if let Some(stuff) = &quick_options_stuff {
                () = msg![env; (stuff.main_view) setHidden:true];
            }
        } else if std::mem::take(&mut host_obj.quick_options_prev_page) {
            if let Some(stuff) = &quick_options_stuff {
                if quick_options_page != 0 {
                    quick_options_page -= 1;
                    update_quick_options_page(env, stuff, quick_options_page);
                }
            }
        } else if std::mem::take(&mut host_obj.quick_options_next_page) {
            if let Some(stuff) = &quick_options_stuff {
                if quick_options_page + 1 < stuff.page_count {
                    quick_options_page += 1;
                    update_quick_options_page(env, stuff, quick_options_page);
                }
            }
        } else {
            let switch_to_arm32 = std::mem::take(&mut host_obj.arm32_apps);
            let switch_to_arm64 = std::mem::take(&mut host_obj.arm64_apps);
            if switch_to_arm32 || switch_to_arm64 {
                let architecture = if switch_to_arm64 {
                    AppArchitecture::Arm64
                } else {
                    AppArchitecture::Arm32
                };
                selected_architecture = architecture;
                if let Ok(all_apps) = apps.as_mut() {
                    selected_app_indices = app_indices_for_architecture(all_apps, architecture);
                    if let Some(stuff) = &mut icon_grid_stuff {
                        stuff.pages = make_icon_grid_pages(
                            selected_app_indices.len(),
                            stuff.icon_buttons_and_labels.len(),
                        );
                        update_icon_grid(env, stuff, all_apps, &selected_app_indices, 0);
                    }
                }
                if let Some(stuff) = &quick_options_stuff {
                    update_quick_option_buttons(
                        env,
                        &stuff.architecture_buttons,
                        usize::from(selected_architecture == AppArchitecture::Arm64),
                    );
                    () = msg![env; (stuff.main_view) setHidden:true];
                }
            } else if let Some(enabled) = std::mem::take(&mut host_obj.print_fps) {
                quick_options_print_fps = enabled;
            } else if let Some(enabled) = std::mem::take(&mut host_obj.force_composition) {
                quick_options_force_composition = enabled;
            } else if let Some(enabled) = std::mem::take(&mut host_obj.ignore_gl_errors) {
                quick_options_ignore_gl_errors = enabled;
            } else if let Some(enabled) = std::mem::take(&mut host_obj.error_popups) {
                quick_options_error_popups = enabled;
            } else if host_obj.setting_toggled != nil {
                let switch = std::mem::replace(&mut host_obj.setting_toggled, nil);
                let value = host_obj.setting_toggled_value;
                let idx = quick_options_stuff
                    .as_ref()
                    .and_then(|stuff| stuff.app_settings.index_of_switch(switch));
                // Mirror what the system Settings app does: write the value the
                // app asked for, flush it to disk, then leave (apps read these
                // preferences at launch, so the new value applies next run).
                if let Some(entry) = idx.and_then(|i| {
                    quick_options_stuff
                        .as_ref()
                        .and_then(|stuff| stuff.app_settings.toggles.get(i))
                }) {
                    // The picker environment has a fake bundle and filesystem,
                    // so the value can't go through the guest's NSUserDefaults:
                    // write it into the app's own sandbox host-side instead.
                    // The app reads it from there at launch, so stay in the
                    // picker.
                    if let Err(e) = crate::environment::settings_bundle::write_app_pref(
                        &entry.app_path,
                        &entry.toggle.key,
                        if value {
                            &entry.toggle.true_value
                        } else {
                            &entry.toggle.false_value
                        },
                    ) {
                        echo!("{e}");
                    }
                }
            } else if std::mem::take(&mut host_obj.scale_hack_default) {
                quick_options_scale_hack = None;
                if let Some(stuff) = &quick_options_stuff {
                    update_scale_hack_buttons(
                        env,
                        &stuff.scale_hack_buttons,
                        quick_options_scale_hack,
                    );
                }
            } else if std::mem::take(&mut host_obj.scale_hack1) {
                quick_options_scale_hack = Some(NonZeroU32::new(1).unwrap());
                if let Some(stuff) = &quick_options_stuff {
                    update_scale_hack_buttons(
                        env,
                        &stuff.scale_hack_buttons,
                        quick_options_scale_hack,
                    );
                }
            } else if std::mem::take(&mut host_obj.scale_hack2) {
                quick_options_scale_hack = Some(NonZeroU32::new(2).unwrap());
                if let Some(stuff) = &quick_options_stuff {
                    update_scale_hack_buttons(
                        env,
                        &stuff.scale_hack_buttons,
                        quick_options_scale_hack,
                    );
                }
            } else if std::mem::take(&mut host_obj.scale_hack3) {
                quick_options_scale_hack = Some(NonZeroU32::new(3).unwrap());
                if let Some(stuff) = &quick_options_stuff {
                    update_scale_hack_buttons(
                        env,
                        &stuff.scale_hack_buttons,
                        quick_options_scale_hack,
                    );
                }
            } else if std::mem::take(&mut host_obj.scale_hack4) {
                quick_options_scale_hack = Some(NonZeroU32::new(4).unwrap());
                if let Some(stuff) = &quick_options_stuff {
                    update_scale_hack_buttons(
                        env,
                        &stuff.scale_hack_buttons,
                        quick_options_scale_hack,
                    );
                }
            } else if std::mem::take(&mut host_obj.orientation_default) {
                quick_options_orientation = None;
                if let Some(stuff) = &quick_options_stuff {
                    update_orientation_buttons(
                        env,
                        &stuff.orientation_buttons,
                        quick_options_orientation,
                    );
                }
            } else if std::mem::take(&mut host_obj.orientation_portrait_upside_down) {
                quick_options_orientation = Some(DeviceOrientation::PortraitUpsideDown);
                if let Some(stuff) = &quick_options_stuff {
                    update_orientation_buttons(
                        env,
                        &stuff.orientation_buttons,
                        quick_options_orientation,
                    );
                }
            } else if std::mem::take(&mut host_obj.orientation_landscape_left) {
                quick_options_orientation = Some(DeviceOrientation::LandscapeLeft);
                if let Some(stuff) = &quick_options_stuff {
                    update_orientation_buttons(
                        env,
                        &stuff.orientation_buttons,
                        quick_options_orientation,
                    );
                }
            } else if std::mem::take(&mut host_obj.orientation_landscape_right) {
                quick_options_orientation = Some(DeviceOrientation::LandscapeRight);
                if let Some(stuff) = &quick_options_stuff {
                    update_orientation_buttons(
                        env,
                        &stuff.orientation_buttons,
                        quick_options_orientation,
                    );
                }
            } else if std::mem::take(&mut host_obj.region_pulldown_toggle) {
                if let Some(stuff) = &quick_options_stuff {
                    toggle_region_pulldown(env, &stuff.region_pulldown);
                }
            } else if std::mem::take(&mut host_obj.region_pulldown_close) {
                if let Some(stuff) = &quick_options_stuff {
                    set_region_pulldown_visible(env, &stuff.region_pulldown, false);
                }
            } else if std::mem::take(&mut host_obj.region_default) {
                quick_options_country_code = None;
                if let Some(stuff) = &quick_options_stuff {
                    update_region_buttons(env, &stuff.region_pulldown, quick_options_country_code);
                }
            } else if std::mem::take(&mut host_obj.region_us) {
                quick_options_country_code = Some("US");
                if let Some(stuff) = &quick_options_stuff {
                    update_region_buttons(env, &stuff.region_pulldown, quick_options_country_code);
                }
            } else if std::mem::take(&mut host_obj.region_gb) {
                quick_options_country_code = Some("GB");
                if let Some(stuff) = &quick_options_stuff {
                    update_region_buttons(env, &stuff.region_pulldown, quick_options_country_code);
                }
            } else if std::mem::take(&mut host_obj.region_jp) {
                quick_options_country_code = Some("JP");
                if let Some(stuff) = &quick_options_stuff {
                    update_region_buttons(env, &stuff.region_pulldown, quick_options_country_code);
                }
            } else if std::mem::take(&mut host_obj.region_fr) {
                quick_options_country_code = Some("FR");
                if let Some(stuff) = &quick_options_stuff {
                    update_region_buttons(env, &stuff.region_pulldown, quick_options_country_code);
                }
            } else if std::mem::take(&mut host_obj.region_de) {
                quick_options_country_code = Some("DE");
                if let Some(stuff) = &quick_options_stuff {
                    update_region_buttons(env, &stuff.region_pulldown, quick_options_country_code);
                }
            } else if std::mem::take(&mut host_obj.tilt_sensitivity_default) {
                quick_options_tilt_sensitivity = None;
                if let Some(stuff) = &quick_options_stuff {
                    update_tilt_sensitivity_buttons(
                        env,
                        &stuff.tilt_sensitivity_buttons,
                        quick_options_tilt_sensitivity,
                    );
                }
            } else if std::mem::take(&mut host_obj.tilt_sensitivity_half) {
                quick_options_tilt_sensitivity = Some(0.5);
                if let Some(stuff) = &quick_options_stuff {
                    update_tilt_sensitivity_buttons(
                        env,
                        &stuff.tilt_sensitivity_buttons,
                        quick_options_tilt_sensitivity,
                    );
                }
            } else if std::mem::take(&mut host_obj.tilt_sensitivity_three_quarters) {
                quick_options_tilt_sensitivity = Some(0.75);
                if let Some(stuff) = &quick_options_stuff {
                    update_tilt_sensitivity_buttons(
                        env,
                        &stuff.tilt_sensitivity_buttons,
                        quick_options_tilt_sensitivity,
                    );
                }
            } else if std::mem::take(&mut host_obj.tilt_sensitivity_one_and_a_half) {
                quick_options_tilt_sensitivity = Some(1.5);
                if let Some(stuff) = &quick_options_stuff {
                    update_tilt_sensitivity_buttons(
                        env,
                        &stuff.tilt_sensitivity_buttons,
                        quick_options_tilt_sensitivity,
                    );
                }
            } else if std::mem::take(&mut host_obj.tilt_sensitivity_double) {
                quick_options_tilt_sensitivity = Some(2.0);
                if let Some(stuff) = &quick_options_stuff {
                    update_tilt_sensitivity_buttons(
                        env,
                        &stuff.tilt_sensitivity_buttons,
                        quick_options_tilt_sensitivity,
                    );
                }
            } else if let Some(enabled) = std::mem::take(&mut host_obj.analog_stick_tilt_controls) {
                quick_options_analog_stick_tilt_controls = enabled;
            } else if let Some(enabled) = std::mem::take(&mut host_obj.network) {
                quick_options_network = enabled;
            } else if let Some(fullscreen) = std::mem::take(&mut host_obj.fullscreen) {
                quick_options_fullscreen = match fullscreen {
                    false => None,
                    true => Some(()),
                };
            }
        }
    };

    // Apply user-specified overrides
    if let Some(scale_hack) = quick_options_scale_hack {
        option_args.push(format!("--scale-hack={}", scale_hack.get()));
    }
    if let Some(orientation) = quick_options_orientation {
        option_args.push(
            match orientation {
                DeviceOrientation::LandscapeLeft => "--landscape-left",
                DeviceOrientation::LandscapeRight => "--landscape-right",
                DeviceOrientation::PortraitUpsideDown => "--upside-down",
                _ => todo!(),
            }
            .to_string(),
        );
    }
    if let Some(()) = quick_options_fullscreen {
        option_args.push("--fullscreen".to_string());
    }
    if !quick_options_analog_stick_tilt_controls {
        option_args.push("--disable-analog-stick-tilt-controls".to_string());
    }
    if let Some(sensitivity) = quick_options_tilt_sensitivity {
        option_args.push(format!("--tilt-sensitivity={sensitivity}"));
    }
    if let Some(country_code) = quick_options_country_code {
        option_args.push(format!("--country-code={country_code}"));
        // Apps often localize by language rather than country, so a region
        // pick also implies the matching preferred language.
        let language = match country_code {
            "GB" | "US" => "en",
            "JP" => "ja",
            "FR" => "fr",
            "DE" => "de",
            _ => unreachable!(),
        };
        option_args.push(format!("--preferred-languages={language}"));
    }
    if quick_options_network {
        option_args.push("--allow-network-access".to_string());
    }
    if quick_options_print_fps {
        option_args.push("--print-fps".to_string());
    }
    if quick_options_force_composition {
        option_args.push("--force-composition".to_string());
    }
    if quick_options_ignore_gl_errors {
        option_args.push("--ignore-gl-errors".to_string());
    }
    if !quick_options_error_popups {
        option_args.push("--no-error-popup".to_string());
    }

    // Return the environment so some parts of it can be salvaged.
    (app_path, option_args)
}

const ICON_SIZE: CGSize = CGSize {
    width: 57.0,
    height: 57.0,
};

enum TappedIcon {
    App(usize),
    ChangePage(usize),
}

fn app_indices_for_architecture(apps: &[AppInfo], architecture: AppArchitecture) -> Vec<usize> {
    apps.iter()
        .enumerate()
        .filter_map(|(idx, app)| (app.architecture == architecture).then_some(idx))
        .collect()
}

struct IconGridStuff {
    icon_buttons_and_labels: Vec<(id, id)>,
    placeholder_icon: Option<id>,
    prev_icon: Option<id>,
    next_icon: Option<id>,
    pages: Vec<std::ops::Range<usize>>,
    icon_map: HashMap<id, TappedIcon>,
}

fn make_icon_grid(
    env: &mut Environment,
    delegate: id,
    main_view: id,
    app_frame: CGRect,
    total_app_count: usize,
    have_wallpaper: bool,
) -> IconGridStuff {
    let num_cols = 4;
    let num_cols_f = num_cols as CGFloat;
    let num_rows = 4;
    let label_size = CGSize {
        width: 74.0,
        height: 13.0,
    };
    let icon_gap_x: CGFloat = 19.0;
    let icon_gap_y: CGFloat = 4.0 + label_size.height + 14.0;
    let icon_grid_width = (ICON_SIZE.width * num_cols_f) + icon_gap_x * (num_cols_f - 1.0);
    let icon_grid_origin = CGPoint {
        x: (app_frame.size.width - icon_grid_width) / 2.0,
        y: 12.0,
    };

    let icon_tapped_sel = env.objc.lookup_selector("iconTapped:").unwrap();

    let mut icon_buttons_and_labels = Vec::new();

    for i in 0..(num_cols * num_rows) {
        let col = i % num_cols;
        let row = i / num_cols;

        // Rounding is needed here to avoid a blurry or offset image.
        let icon_frame = CGRect {
            origin: CGPoint {
                x: (icon_grid_origin.x + (col as CGFloat) * (ICON_SIZE.width + icon_gap_x)).round(),
                y: (icon_grid_origin.y + (row as CGFloat) * (ICON_SIZE.height + icon_gap_y))
                    .round(),
            },
            size: ICON_SIZE,
        };
        let icon_button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
        () = msg![env; icon_button setFrame:icon_frame];
        let image_view: id = msg![env; icon_button imageView];
        let bounds: CGRect = msg![env; icon_button bounds];
        () = msg![env; image_view setFrame:bounds];
        () = msg![env; icon_button addTarget:delegate
                                      action:icon_tapped_sel
                            forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; main_view addSubview:icon_button];

        // Rounding is needed here to avoid blurry text.
        let label_frame = CGRect {
            origin: CGPoint {
                x: (icon_frame.origin.x - (label_size.width - ICON_SIZE.width) / 2.0).round(),
                y: (icon_frame.origin.y + ICON_SIZE.height + 4.0).round(),
            },
            size: label_size,
        };
        let label: id = msg_class![env; UILabel alloc];
        let label: id = msg![env; label initWithFrame:label_frame];
        () = msg![env; label setTextAlignment:UITextAlignmentCenter];
        let font_size: CGFloat = label_size.height - 2.0;
        let font: id = if have_wallpaper {
            msg_class![env; UIFont systemFontOfSize:font_size]
        } else {
            msg_class![env; UIFont boldSystemFontOfSize:font_size]
        };
        () = msg![env; label setFont:font];
        let text_color: id = if have_wallpaper {
            msg_class![env; UIColor whiteColor]
        } else {
            msg_class![env; UIColor lightGrayColor]
        };
        () = msg![env; label setTextColor:text_color];
        let bg_color: id = msg_class![env; UIColor clearColor];
        () = msg![env; label setBackgroundColor:bg_color];
        () = msg![env; main_view addSubview:label];

        icon_buttons_and_labels.push((icon_button, label));
    }

    // TODO: Use UIScrollView pagination and UIPageControl once available.
    let pages = make_icon_grid_pages(total_app_count, icon_buttons_and_labels.len());

    IconGridStuff {
        icon_buttons_and_labels,
        placeholder_icon: None,
        prev_icon: None,
        next_icon: None,
        pages,
        icon_map: HashMap::new(),
    }
}

fn make_icon_grid_pages(total_app_count: usize, icon_count: usize) -> Vec<std::ops::Range<usize>> {
    if total_app_count == 0 {
        return std::iter::once(0..0).collect();
    }
    let mut pages = Vec::new();
    let mut start = 0;
    while start < total_app_count {
        let mut end = start + icon_count;
        if start > 0 {
            end -= 1;
        }
        if end < total_app_count {
            end -= 1;
        } else {
            end = total_app_count;
        }
        pages.push(start..end);
        start = end;
    }
    pages
}

fn make_icon_from_glyph(
    env: &mut Environment,
    glyph: char,
    font_size: CGFloat,
    baseline_offset: CGFloat,
    bg_color: (CGFloat, CGFloat, CGFloat, CGFloat),
) -> id {
    let color_space = CGColorSpaceCreateDeviceRGB(env);
    let context = CGBitmapContextCreate(
        env,
        Ptr::null(),
        ICON_SIZE.width as u32,
        ICON_SIZE.height as u32,
        8,
        4 * (ICON_SIZE.width as u32),
        color_space,
        kCGImageAlphaPremultipliedLast,
    );
    UIGraphicsPushContext(env, context);

    // Compensate for row order inversion
    CGContextTranslateCTM(env, context, 0.0, ICON_SIZE.height);
    CGContextScaleCTM(env, context, 1.0, -1.0);

    let (r, g, b, a) = bg_color;
    CGContextSetRGBFillColor(env, context, r, g, b, a);
    CGContextFillRect(
        env,
        context,
        CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: ICON_SIZE,
        },
    );

    let font: id = msg_class![env; UIFont systemFontOfSize:font_size];
    let glyph_string: id = ns_string::from_rust_string(env, [glyph].into_iter().collect());
    let glyph_size: CGSize = msg![env; glyph_string sizeWithFont:font];
    CGContextSetRGBFillColor(env, context, 1.0, 1.0, 1.0, 1.0); // white
    let glyph_origin = CGPoint {
        x: ICON_SIZE.width / 2.0 - glyph_size.width / 2.0,
        y: ICON_SIZE.height / 2.0 - glyph_size.height / 2.0 + baseline_offset,
    };
    let _: CGSize = msg![env; glyph_string drawAtPoint:glyph_origin withFont:font];
    release(env, glyph_string);

    UIGraphicsPopContext(env);

    let cg_image = CGBitmapContextCreateImage(env, context);
    // This radius should match the one in src/bundle.rs.
    cg_image::borrow_image_mut(&mut env.objc, cg_image).round_corners(
        (10.0 / 57.0) * ICON_SIZE.width,
        /* four_corners: */ true,
        /* add_sheen: */ true,
    );
    CGContextRelease(env, context);

    let ui_image: id = msg_class![env; UIImage imageWithCGImage:cg_image];
    release(env, cg_image);

    ui_image
}

/// Frutiger Aero–style glossy button background, generated at a canonical
/// size and stretched to the button's bounds by the background image view.
/// The gradient, top sheen and rounded corners are drawn per pixel row, since
/// the CGContext implementation has no gradient primitives.
fn make_frutiger_button_image(env: &mut Environment, selected: bool) -> id {
    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 30;

    let color_space = CGColorSpaceCreateDeviceRGB(env);
    let context = CGBitmapContextCreate(
        env,
        Ptr::null(),
        WIDTH,
        HEIGHT,
        8,
        4 * WIDTH,
        color_space,
        kCGImageAlphaPremultipliedLast,
    );
    UIGraphicsPushContext(env, context);

    // Compensate for row order inversion (y=0 becomes the top row)
    CGContextTranslateCTM(env, context, 0.0, HEIGHT as CGFloat);
    CGContextScaleCTM(env, context, 1.0, -1.0);

    let (top, bottom) = if selected {
        // Shiny aqua: light cyan to deep blue
        ((0.75, 0.95, 1.0), (0.03, 0.42, 0.85))
    } else {
        // Frosted graphite: pale silver to slate
        ((0.88, 0.91, 0.95), (0.36, 0.43, 0.53))
    };
    for y in 0..HEIGHT {
        let t = y as f32 / (HEIGHT - 1) as f32;
        CGContextSetRGBFillColor(
            env,
            context,
            (top.0 + (bottom.0 - top.0) * t) as CGFloat,
            (top.1 + (bottom.1 - top.1) * t) as CGFloat,
            (top.2 + (bottom.2 - top.2) * t) as CGFloat,
            1.0,
        );
        CGContextFillRect(
            env,
            context,
            CGRect {
                origin: CGPoint {
                    x: 0.0,
                    y: y as CGFloat,
                },
                size: CGSize {
                    width: WIDTH as CGFloat,
                    height: 1.0,
                },
            },
        );
    }
    // The classic glass sheen: a white fade across the top half
    for y in 0..HEIGHT / 2 {
        let t = y as f32 / (HEIGHT / 2) as f32;
        let alpha = 0.55 * (1.0 - t) + 0.03;
        CGContextSetRGBFillColor(env, context, 1.0, 1.0, 1.0, alpha as CGFloat);
        CGContextFillRect(
            env,
            context,
            CGRect {
                origin: CGPoint {
                    x: 0.0,
                    y: y as CGFloat,
                },
                size: CGSize {
                    width: WIDTH as CGFloat,
                    height: 1.0,
                },
            },
        );
    }

    UIGraphicsPopContext(env);

    let cg_image = CGBitmapContextCreateImage(env, context);
    cg_image::borrow_image_mut(&mut env.objc, cg_image).round_corners(
        5.0, /* four_corners: */ true, /* add_sheen: */ false,
    );
    CGContextRelease(env, context);

    let ui_image: id = msg_class![env; UIImage imageWithCGImage:cg_image];
    release(env, cg_image);

    ui_image
}

fn make_frutiger_panel_image(env: &mut Environment, width: u32, height: u32) -> id {
    let color_space = CGColorSpaceCreateDeviceRGB(env);
    let context = CGBitmapContextCreate(
        env,
        Ptr::null(),
        width,
        height,
        8,
        4 * width,
        color_space,
        kCGImageAlphaPremultipliedLast,
    );
    UIGraphicsPushContext(env, context);

    // Compensate for row order inversion (y=0 becomes the top row)
    CGContextTranslateCTM(env, context, 0.0, height as CGFloat);
    CGContextScaleCTM(env, context, 1.0, -1.0);

    // Aqua glass: pale cyan fading into deep Vista blue.
    let (top, bottom) = ((0.72, 0.93, 1.0), (0.16, 0.46, 0.78));
    for y in 0..height {
        let t = y as f32 / (height - 1) as f32;
        CGContextSetRGBFillColor(
            env,
            context,
            (top.0 + (bottom.0 - top.0) * t) as CGFloat,
            (top.1 + (bottom.1 - top.1) * t) as CGFloat,
            (top.2 + (bottom.2 - top.2) * t) as CGFloat,
            1.0,
        );
        CGContextFillRect(
            env,
            context,
            CGRect {
                origin: CGPoint {
                    x: 0.0,
                    y: y as CGFloat,
                },
                size: CGSize {
                    width: width as CGFloat,
                    height: 1.0,
                },
            },
        );
    }
    // The classic glass sheen: a white fade across the top half.
    let sheen_height = height / 2;
    for y in 0..sheen_height {
        let t = y as f32 / sheen_height as f32;
        let alpha = 0.5 * (1.0 - t) + 0.05;
        CGContextSetRGBFillColor(env, context, 1.0, 1.0, 1.0, alpha as CGFloat);
        CGContextFillRect(
            env,
            context,
            CGRect {
                origin: CGPoint {
                    x: 0.0,
                    y: y as CGFloat,
                },
                size: CGSize {
                    width: width as CGFloat,
                    height: 1.0,
                },
            },
        );
    }

    UIGraphicsPopContext(env);

    let cg_image = CGBitmapContextCreateImage(env, context);
    cg_image::borrow_image_mut(&mut env.objc, cg_image).round_corners(
        10.0, /* four_corners: */ true, /* add_sheen: */ false,
    );
    CGContextRelease(env, context);

    let ui_image: id = msg_class![env; UIImage imageWithCGImage:cg_image];
    release(env, cg_image);

    ui_image
}

fn make_region_pulldown(
    env: &mut Environment,
    delegate: id,
    super_view: id,
    super_frame: CGRect,
    trigger: id,
    nav_height: CGFloat,
) -> RegionPulldownStuff {
    let item_height: CGFloat = 30.0;
    let gap: CGFloat = 2.0;
    let padding: CGFloat = 8.0;
    let panel_width: CGFloat = 170.0;
    let panel_height = (REGION_ITEM_TITLES.len() as CGFloat) * item_height
        + padding * 2.0
        + (REGION_ITEM_TITLES.len() as CGFloat - 1.0) * gap;

    // An invisible full-screen button that closes the pulldown when the user
    // taps outside of it.
    let scrim: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
    let scrim_frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: super_frame.size,
    };
    () = msg![env; scrim setFrame:scrim_frame];
    let close_selector = env.objc.lookup_selector("regionPulldownClose").unwrap();
    () = msg![env; scrim addTarget:delegate
                             action:close_selector
                   forControlEvents:UIControlEventTouchUpInside];
    () = msg![env; super_view addSubview:scrim];

    // Position the panel just below the trigger, or above it if there's no
    // room left. The trigger lives in a page view that fills the whole
    // panel, so its frame matches this coordinate space.
    let trigger_frame: CGRect = msg![env; trigger frame];
    let below = trigger_frame.origin.y + trigger_frame.size.height + 4.0;
    let y = if below + panel_height <= super_frame.size.height - nav_height {
        below
    } else {
        trigger_frame.origin.y - panel_height - 4.0
    };
    let panel_frame = CGRect {
        origin: CGPoint {
            x: (super_frame.size.width - panel_width) / 2.0,
            y,
        },
        size: CGSize {
            width: panel_width,
            height: panel_height,
        },
    };

    // A plain UIView, NOT a UIButton: UIButton routes all touches straight
    // to itself, which would stop the item buttons from ever being tapped.
    let panel: id = msg_class![env; UIView alloc];
    let panel: id = msg![env; panel initWithFrame:panel_frame];
    let panel_image = make_frutiger_panel_image(env, panel_width as u32, panel_height as u32);
    let panel_image_view: id = msg_class![env; UIImageView alloc];
    let panel_image_view: id = msg![env; panel_image_view initWithImage:panel_image];
    release(env, panel_image);
    // Let touches fall through to the item buttons.
    () = msg![env; panel_image_view setUserInteractionEnabled:false];
    () = msg![env; panel addSubview:panel_image_view];
    () = msg![env; panel setHidden:true];
    () = msg![env; super_view addSubview:panel];

    let mut item_buttons = Vec::new();
    for (idx, &title) in REGION_ITEM_TITLES.iter().enumerate() {
        let item_frame = CGRect {
            origin: CGPoint {
                x: padding,
                y: padding + (idx as CGFloat) * (item_height + gap),
            },
            size: CGSize {
                width: panel_width - padding * 2.0,
                height: item_height,
            },
        };
        let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeCustom];
        let text = ns_string::get_static_str(env, title);
        () = msg![env; button setTitle:text forState:UIControlStateNormal];
        () = msg![env; button setFrame:item_frame];
        // FIXME: manually calling layoutSubviews shouldn't be needed?
        () = msg![env; button layoutSubviews];
        let selector = env
            .objc
            .lookup_selector(REGION_ITEM_SELECTORS[idx])
            .unwrap();
        () = msg![env; button addTarget:delegate
                                 action:selector
                       forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; panel addSubview:button];
        item_buttons.push(button);
    }

    RegionPulldownStuff {
        trigger,
        scrim,
        panel,
        item_buttons,
    }
}

fn set_region_pulldown_visible(
    env: &mut Environment,
    pulldown: &RegionPulldownStuff,
    visible: bool,
) {
    let scrim = pulldown.scrim;
    let panel = pulldown.panel;
    () = msg![env; scrim setHidden:(!visible)];
    () = msg![env; panel setHidden:(!visible)];
}

fn toggle_region_pulldown(env: &mut Environment, pulldown: &RegionPulldownStuff) {
    let panel = pulldown.panel;
    let hidden: bool = msg![env; panel isHidden];
    set_region_pulldown_visible(env, pulldown, hidden);
}

fn update_icon_grid(
    env: &mut Environment,
    icon_grid_stuff: &mut IconGridStuff,
    apps: &mut [AppInfo],
    app_indices: &[usize],
    page_idx: usize,
) {
    icon_grid_stuff.icon_map.clear();

    let app_idx_range = icon_grid_stuff.pages[page_idx].clone();
    let have_prev_icon = page_idx != 0;
    let have_next_icon = app_idx_range.end != app_indices.len();

    let mut icon_iter = icon_grid_stuff.icon_buttons_and_labels.iter();

    if have_prev_icon {
        let &(icon_button, label) = icon_iter.next().unwrap();
        let image = *icon_grid_stuff.prev_icon.get_or_insert_with(|| {
            make_icon_from_glyph(env, '←', 50.0, -9.0, (0.25, 0.25, 0.25, 1.0))
        });
        () = msg![env; icon_button setImage:image forState:UIControlStateNormal];
        () = msg![env; label setText:(ns_string::get_static_str(env, ""))];
        icon_grid_stuff
            .icon_map
            .insert(icon_button, TappedIcon::ChangePage(page_idx - 1));
    }

    for visible_idx in app_idx_range.clone() {
        let app_idx = app_indices[visible_idx];
        let app = &mut apps[app_idx];

        let &(icon_button, label) = icon_iter.next().unwrap();

        if let Some(icon) = app.icon.take() {
            let image = cg_image::from_image(env, icon);
            let image: id = msg_class![env; UIImage imageWithCGImage:image];
            app.icon_ui_image = Some(image);
        }

        let image = app.icon_ui_image.unwrap_or_else(|| {
            *icon_grid_stuff.placeholder_icon.get_or_insert_with(|| {
                make_icon_from_glyph(env, '?', 40.0, 0.0, (0.5, 0.5, 0.5, 1.0))
            })
        });
        () = msg![env; icon_button setImage:image forState:UIControlStateNormal];

        let text = *app
            .display_name_ns_string
            .get_or_insert_with(|| ns_string::from_rust_string(env, app.display_name.clone()));
        () = msg![env; label setText:text];

        icon_grid_stuff
            .icon_map
            .insert(icon_button, TappedIcon::App(app_idx));
    }

    if have_next_icon {
        let &(icon_button, label) = icon_iter.next().unwrap();
        let image = *icon_grid_stuff.next_icon.get_or_insert_with(|| {
            make_icon_from_glyph(env, '→', 50.0, -9.0, (0.25, 0.25, 0.25, 1.0))
        });
        () = msg![env; icon_button setImage:image forState:UIControlStateNormal];
        () = msg![env; label setText:(ns_string::get_static_str(env, ""))];
        icon_grid_stuff
            .icon_map
            .insert(icon_button, TappedIcon::ChangePage(page_idx + 1));
    }

    // There may be remaining spaces might need to be blanked.
    for &(icon_button, label) in icon_iter {
        () = msg![env; icon_button setImage:nil forState:UIControlStateNormal];
        () = msg![env; label setText:(ns_string::get_static_str(env, ""))];
    }
}

fn make_button_row(
    env: &mut Environment,
    delegate: id,
    super_view: id,
    super_view_size: CGSize,
    buttons_row_center: CGFloat,
    buttons: &[(&'static str, &'static str)],
    font_size: Option<CGFloat>,
) -> Vec<id> {
    let margin = 10.0;

    let button_size = CGSize {
        width: (super_view_size.width - margin) / (buttons.len() as CGFloat) - margin,
        height: 30.0,
    };
    let mut button_frame = CGRect {
        origin: CGPoint {
            x: margin,
            y: buttons_row_center - button_size.height / 2.0,
        },
        size: button_size,
    };

    let mut ui_buttons = Vec::new();
    for (title_text, selector) in buttons {
        let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeRoundedRect];
        let text = ns_string::get_static_str(env, title_text);
        () = msg![env; button setTitle:text forState:UIControlStateNormal];
        () = msg![env; button setFrame:button_frame];
        // FIXME: manually calling layoutSubviews shouldn't be needed?
        () = msg![env; button layoutSubviews];

        if let Some(font_size) = font_size {
            let label: id = msg![env; button titleLabel];
            let font: id = msg_class![env; UIFont systemFontOfSize:font_size];
            () = msg![env; label setFont:font];
        }

        let selector = env.objc.lookup_selector(selector).unwrap();
        () = msg![env; button addTarget:delegate
                                 action:selector
                       forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; super_view addSubview:button];

        button_frame.origin.x += button_size.width + margin;
        ui_buttons.push(button);
    }
    ui_buttons
}

struct CopyrightInfoStuff {
    main_view: id,
    text_frame: CGRect,
    text_label: id,
    font: id,
    pages: Vec<(std::ops::Range<usize>, CGFloat)>,
    last_page_idx: Option<usize>,
    prev_page_button: id,
    next_page_button: id,
}

fn setup_copyright_info(
    env: &mut Environment,
    delegate: id,
    super_view: id,
    app_frame: CGRect,
) -> CopyrightInfoStuff {
    let main_frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: app_frame.size,
    };

    let divider = main_frame.size.height - 40.0;

    // Container for all the other stuff

    let main_view: id = msg_class![env; UIView alloc];
    let main_view: id = msg![env; main_view initWithFrame:main_frame];
    // TODO: Isn't white the default?
    let bg_color: id = msg_class![env; UIColor whiteColor];
    () = msg![env; main_view setBackgroundColor:bg_color];
    // This main_view is hidden until the copyright info button is tapped.
    () = msg![env; main_view setHidden:true];
    () = msg![env; super_view addSubview:main_view];

    // UILabel that will display part of the copyright text

    let padding = 10.0;
    let text_frame = CGRect {
        origin: CGPoint {
            x: padding,
            y: padding,
        },
        size: CGSize {
            width: app_frame.size.width - padding * 2.0,
            height: divider - padding * 2.0,
        },
    };

    let text_label: id = msg_class![env; UILabel alloc];
    let text_label: id = msg![env; text_label initWithFrame:text_frame];
    () = msg![env; text_label setNumberOfLines:0]; // unlimited
    let text_color: id = msg_class![env; UIColor blackColor];
    () = msg![env; text_label setTextColor:text_color];
    let bg_color: id = msg_class![env; UIColor clearColor];
    () = msg![env; text_label setBackgroundColor:bg_color];
    let font_size: CGFloat = 16.0;
    let font: id = msg_class![env; UIFont systemFontOfSize:font_size];
    () = msg![env; text_label setFont:font];
    () = msg![env; main_view addSubview:text_label];

    // Navigation

    let buttons_row_center = (main_frame.size.height + divider) / 2.0;
    let buttons = make_button_row(
        env,
        delegate,
        main_view,
        main_frame.size,
        buttons_row_center,
        &[
            ("↑", "copyrightInfoPrevPage"),
            ("↓", "copyrightInfoNextPage"),
            ("×", "copyrightInfoHide"),
        ],
        Some(30.0),
    );

    CopyrightInfoStuff {
        main_view,
        text_frame,
        text_label,
        font,
        pages: Vec::new(),
        last_page_idx: None,
        prev_page_button: buttons[0],
        next_page_button: buttons[1],
    }
}

fn change_copyright_page(
    env: &mut Environment,
    copyright_info_stuff: &mut CopyrightInfoStuff,
    copyright_info_text: &str,
    page_idx: usize,
) {
    // TODO: Eventually this should be ripped out and replaced with a scrolling
    // UITextView, once that's implemented.

    let &mut CopyrightInfoStuff {
        text_frame,
        text_label,
        font,
        ref mut pages,
        ref mut last_page_idx,
        prev_page_button,
        next_page_button,
        ..
    } = copyright_info_stuff;

    // Lazily lay out pages of text as needed.

    if page_idx == pages.len() {
        let mut page_start = pages.last().map_or(0, |page| page.0.end);
        while copyright_info_text[page_start..].starts_with([' ', '\n', '\r']) {
            page_start += 1;
        }
        let mut page_height = 0.0;
        let page_end = loop {
            let mut line_start = page_start;
            while line_start < copyright_info_text.len() {
                let is_first_line = line_start == page_start;

                let line_end = if let Some(i) = copyright_info_text[line_start..].find('\n') {
                    line_start + i + 1
                } else {
                    copyright_info_text.len()
                };

                let line = &copyright_info_text[line_start..line_end];

                // Force pagination before headings (in Dynarmic's license text)
                if !is_first_line && line.starts_with("###") {
                    break;
                }

                let line_temp = ns_string::from_rust_string(env, line.to_string());
                let line_size: CGSize = msg![env; line_temp sizeWithFont:font
                                                       constrainedToSize:(text_frame.size)];
                // Avoid accumulation of old line strings.
                release(env, line_temp);

                if page_height + line_size.height > text_frame.size.height {
                    break;
                }

                page_height += line_size.height;
                line_start = line_end;

                // Force pagination after dividers
                if !is_first_line && line.starts_with("---") {
                    break;
                }
            }
            let page_end = line_start;
            assert!(page_start != page_end);

            // Avoid entirely blank pages
            if copyright_info_text[page_start..page_end].trim() == "" {
                page_start = page_end;
            } else {
                break page_end;
            }
        };
        assert!(page_start != page_end);
        pages.push((page_start..page_end, page_height));
        if page_end == copyright_info_text.len() {
            *last_page_idx = Some(page_idx);
        }
    }

    // Actually display the page

    let (page, page_height) = pages[page_idx].clone();
    let page = &copyright_info_text[page];

    let page: id = ns_string::from_rust_string(env, page.to_string());
    () = msg![env; text_label setText:page];
    // Avoid accumulation of old page strings.
    release(env, page);

    // UILabel always vertically centers text. Work around that by resizing it.
    let label_frame = CGRect {
        origin: text_frame.origin,
        size: CGSize {
            width: text_frame.size.width,
            // The page height is slightly off, a little padding is needed.
            height: page_height + 10.0,
        },
    };
    () = msg![env; text_label setFrame:label_frame];

    () = msg![env; prev_page_button setHidden:(page_idx == 0)];
    () = msg![env; next_page_button setHidden:(Some(page_idx) == *last_page_idx)];
}

enum RowKind {
    Label(&'static str),
    Buttons(&'static [(&'static str, &'static str)], Option<CGFloat>),
    Switch(&'static str, bool),
}

const REGION_ITEM_TITLES: [&str; 6] = ["Default", "US", "GB", "JP", "FR", "DE"];
const REGION_ITEM_SELECTORS: [&str; 6] = [
    "regionDefault",
    "regionUS",
    "regionGB",
    "regionJP",
    "regionFR",
    "regionDE",
];

struct RegionPulldownStuff {
    trigger: id,
    scrim: id,
    panel: id,
    item_buttons: Vec<id>,
}

struct QuickOptionsStuff {
    main_view: id,
    page_views: Vec<id>,
    page_label: id,
    scale_hack_buttons: [id; 5],
    orientation_buttons: [id; 4],
    region_pulldown: RegionPulldownStuff,
    tilt_sensitivity_buttons: [id; 5],
    architecture_buttons: [id; 2],
    app_settings: AppSettingsStuff,
    page_count: usize,
}

fn setup_quick_options(
    env: &mut Environment,
    delegate: id,
    super_view: id,
    app_frame: CGRect,
    apps: &[AppInfo],
) -> QuickOptionsStuff {
    // UIView*
    let main_frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: app_frame.size,
    };

    // Container for all the other stuff

    let main_view: id = msg_class![env; UIView alloc];
    let main_view: id = msg![env; main_view initWithFrame:main_frame];
    // TODO: Isn't white the default?
    let bg_color: id = msg_class![env; UIColor whiteColor];
    () = msg![env; main_view setBackgroundColor:bg_color];
    // This main_view is hidden until the copyright info button is tapped.
    () = msg![env; main_view setHidden:true];
    () = msg![env; super_view addSubview:main_view];

    let divider = 40.0;

    let page1 = vec![
        RowKind::Label("Scale hack"),
        RowKind::Buttons(
            &[
                ("Default", "scaleHackDefault"),
                ("Off", "scaleHack1"),
                ("2×", "scaleHack2"),
                ("3×", "scaleHack3"),
                ("4×", "scaleHack4"),
            ],
            None,
        ),
        RowKind::Label("Orientation"),
        RowKind::Buttons(
            &[
                ("Default", "orientationDefault"),
                ("←", "orientationLandscapeLeft"),
                ("→", "orientationLandscapeRight"),
                ("↓", "orientationPortraitUpsideDown"),
            ],
            None,
        ),
        RowKind::Buttons(
            &[("Region: Default \u{25BC}", "regionPulldownToggle")],
            Some(16.0),
        ),
    ];
    let mut page2 = vec![
        RowKind::Label("Network access"),
        RowKind::Switch("network:", false),
        RowKind::Label("Use analog sticks for tilt controls"),
        RowKind::Switch("analogStickTiltControls:", true),
        RowKind::Label("Tilt sensitivity"),
        RowKind::Buttons(
            &[
                ("Default", "tiltSensitivityDefault"),
                ("½×", "tiltSensitivityHalf"),
                ("¾×", "tiltSensitivityThreeQuarters"),
                ("1½×", "tiltSensitivityOneAndAHalf"),
                ("2×", "tiltSensitivityDouble"),
            ],
            None,
        ),
    ];
    if crate::window::Window::rotatable_fullscreen() {
        // Fullscreen option doesn't make sense on always-fullscreen platforms
        page2.push(RowKind::Label("Fullscreen (override)"));
        page2.push(RowKind::Switch("fullscreen:", false));
    }
    let page3 = vec![
        RowKind::Label("App architecture"),
        RowKind::Buttons(&[("ARM32", "arm32Apps"), ("ARM64", "arm64Apps")], None),
        RowKind::Label("Show FPS (in console)"),
        RowKind::Switch("printFps:", false),
        RowKind::Label("Force composition"),
        RowKind::Switch("forceComposition:", false),
        RowKind::Label("Ignore GL errors"),
        RowKind::Switch("ignoreGlErrors:", false),
        RowKind::Label("Error pop-ups"),
        RowKind::Switch("errorPopups:", true),
    ];
    let pages = [page1, page2, page3];
    // Every app that ships a `Settings.bundle` gets extra pages exposing its
    // "device Settings" toggles, since there's no Settings app to host them.
    let toggles: Vec<AppToggle> = apps
        .iter()
        .flat_map(|app| {
            crate::environment::settings_bundle::load_toggles(&app.path)
                .into_iter()
                .map(move |toggle| AppToggle {
                    app_path: app.path.clone(),
                    app_name: app.display_name.clone(),
                    toggle,
                })
        })
        .collect();
    let pages = if toggles.is_empty() {
        Pages::Fixed(pages)
    } else {
        Pages::WithSettings(pages, toggles.clone())
    };
    let page_count = match &pages {
        Pages::Fixed(pages) => pages.len(),
        Pages::WithSettings(pages, toggles) => pages.len() + toggles.len(),
    };
    let nav_height: CGFloat = 50.0;

    let mut button_rows = Vec::new();
    let mut page_views = Vec::new();
    let mut app_settings_switches = Vec::new();
    for page_idx in 0..page_count {
        // Settings toggles get one page each: one label plus one switch does
        // not fit proportionally with the other pages, and one-per-page keeps
        // the switch big enough to hit on a phone.
        let settings_toggle = match &pages {
            Pages::WithSettings(pages, toggles) if page_idx >= pages.len() => {
                Some(&toggles[page_idx - pages.len()])
            }
            _ => None,
        };
        let page: &[RowKind] = match &pages {
            Pages::Fixed(pages) => &pages[page_idx],
            Pages::WithSettings(pages, _) => pages.get(page_idx).map_or(&[][..], |p| &p[..]),
        };

        // Container view for this page, so pages can be shown and hidden
        // independently.
        let page_view: id = msg_class![env; UIView alloc];
        let page_view: id = msg![env; page_view initWithFrame:main_frame];
        let bg_color: id = msg_class![env; UIColor clearColor];
        () = msg![env; page_view setBackgroundColor:bg_color];
        // Only the first page is visible initially.
        let hidden = !page_views.is_empty();
        () = msg![env; page_view setHidden:hidden];
        () = msg![env; main_view addSubview:page_view];
        page_views.push(page_view);

        if let Some(entry) = settings_toggle {
            // App name and the toggle's own title, then the switch.
            let name = ns_string::from_rust_string(env, entry.app_name.clone());
            let name_label = make_centered_label(
                env,
                page_view,
                name,
                main_frame.size,
                main_frame.size.height / 2.0 - 40.0,
            );
            let _ = name_label;
            let box_title = format!("“{}”", entry.toggle.title);
            let title = ns_string::from_rust_string(env, box_title);
            make_centered_label(
                env,
                page_view,
                title,
                main_frame.size,
                main_frame.size.height / 2.0 - 10.0,
            );

            let switch_frame = CGRect {
                origin: CGPoint {
                    x: main_frame.size.width / 2.0 - 94.0 / 2.0,
                    y: main_frame.size.height / 2.0 + 20.0,
                },
                size: Default::default(),
            };
            let switch: id = msg_class![env; UISwitch alloc];
            let switch: id = msg![env; switch initWithFrame:switch_frame];
            () = msg![env; switch setOn:(entry.toggle.current_value(Some(&entry.app_path)))];
            // Tag the switch with its index so the action method can tell the
            // toggles apart without a separate object per toggle.
            () = msg![env; switch setTag:(app_settings_switches.len() as crate::frameworks::foundation::NSInteger)];
            let selector = env.objc.lookup_selector("settingToggled:").unwrap();
            () = msg![env; switch addTarget:delegate
                                     action:selector
                           forControlEvents:UIControlEventValueChanged];
            () = msg![env; page_view addSubview:switch];
            app_settings_switches.push(switch);
            continue;
        }

        for (i, row) in page.iter().enumerate() {
            let row_center = divider
                + ((1 + i) as CGFloat)
                    * ((main_frame.size.height - divider - nav_height)
                        / ((page.len() + 1) as CGFloat));

            match *row {
                RowKind::Label(text) => {
                    let frame = CGRect {
                        origin: CGPoint {
                            x: 0.0,
                            y: row_center - 30.0 / 2.0,
                        },
                        size: CGSize {
                            width: main_frame.size.width,
                            height: 30.0,
                        },
                    };

                    let label: id = msg_class![env; UILabel alloc];
                    let label: id = msg![env; label initWithFrame:frame];
                    let text = ns_string::get_static_str(env, text);
                    () = msg![env; label setText:text];
                    () = msg![env; label setTextAlignment:UITextAlignmentCenter];
                    () = msg![env; page_view addSubview:label];
                }
                RowKind::Buttons(buttons, font_size) => {
                    button_rows.push(make_button_row(
                        env,
                        delegate,
                        page_view,
                        main_frame.size,
                        row_center,
                        buttons,
                        font_size,
                    ));
                }
                RowKind::Switch(selector, default_state) => {
                    let switch_frame = CGRect {
                        origin: CGPoint {
                            x: main_frame.size.width / 2.0 - 94.0 / 2.0,
                            y: row_center - 27.0 / 2.0,
                        },
                        size: Default::default(),
                    };

                    let switch: id = msg_class![env; UISwitch alloc];
                    let switch: id = msg![env; switch initWithFrame:switch_frame];
                    () = msg![env; switch setOn:default_state];
                    let selector = env.objc.lookup_selector(selector).unwrap();
                    () = msg![env; switch addTarget:delegate
                                             action:selector
                                   forControlEvents:UIControlEventValueChanged];
                    () = msg![env; page_view addSubview:switch];
                }
            }
        }
    }

    // Close button (on top of the page views, so it stays tappable)
    {
        let button_frame = CGRect {
            origin: CGPoint {
                x: main_frame.size.width - 30.0,
                y: 10.0,
            },
            size: CGSize {
                width: 20.0,
                height: 20.0,
            },
        };

        let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeRoundedRect];
        let text = ns_string::get_static_str(env, "×");
        () = msg![env; button setTitle:text forState:UIControlStateNormal];
        () = msg![env; button setFrame:button_frame];
        // FIXME: manually calling layoutSubviews shouldn't be needed?
        () = msg![env; button layoutSubviews];

        let label: id = msg![env; button titleLabel];
        let font: id = msg_class![env; UIFont systemFontOfSize:(30.0 as CGFloat)];
        () = msg![env; label setFont:font];

        let selector = env.objc.lookup_selector("quickOptionsHide").unwrap();
        () = msg![env; button addTarget:delegate
                                 action:selector
                       forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; main_view addSubview:button];
    }

    // Page navigation bar
    let page_label_frame = CGRect {
        origin: CGPoint {
            x: main_frame.size.width / 2.0 - 60.0,
            y: main_frame.size.height - nav_height,
        },
        size: CGSize {
            width: 120.0,
            height: nav_height,
        },
    };
    let page_label: id = msg_class![env; UILabel alloc];
    let page_label: id = msg![env; page_label initWithFrame:page_label_frame];
    let text = ns_string::from_rust_string(env, format!("1 / {page_count}"));
    () = msg![env; page_label setText:text];
    () = msg![env; page_label setTextAlignment:UITextAlignmentCenter];
    () = msg![env; main_view addSubview:page_label];

    for (title, x_offset) in [("‹", 10.0), ("›", main_frame.size.width - 50.0)] {
        let button_frame = CGRect {
            origin: CGPoint {
                x: x_offset,
                y: main_frame.size.height - nav_height,
            },
            size: CGSize {
                width: 40.0,
                height: nav_height,
            },
        };
        let button: id = msg_class![env; UIButton buttonWithType:UIButtonTypeRoundedRect];
        let text = ns_string::get_static_str(env, title);
        () = msg![env; button setTitle:text forState:UIControlStateNormal];
        () = msg![env; button setFrame:button_frame];
        // FIXME: manually calling layoutSubviews shouldn't be needed?
        () = msg![env; button layoutSubviews];
        let label: id = msg![env; button titleLabel];
        let font: id = msg_class![env; UIFont systemFontOfSize:(24.0 as CGFloat)];
        () = msg![env; label setFont:font];
        let selector = env
            .objc
            .lookup_selector(if x_offset < 20.0 {
                "quickOptionsPrevPage"
            } else {
                "quickOptionsNextPage"
            })
            .unwrap();
        () = msg![env; button addTarget:delegate
                                 action:selector
                       forControlEvents:UIControlEventTouchUpInside];
        () = msg![env; main_view addSubview:button];
    }

    QuickOptionsStuff {
        main_view,
        page_views,
        page_label,
        scale_hack_buttons: button_rows[0][..].try_into().unwrap(),
        orientation_buttons: button_rows[1][..].try_into().unwrap(),
        region_pulldown: make_region_pulldown(
            env,
            delegate,
            main_view,
            main_frame,
            button_rows[2][0],
            nav_height,
        ),
        tilt_sensitivity_buttons: button_rows[3][..].try_into().unwrap(),
        architecture_buttons: button_rows[4][..].try_into().unwrap(),
        app_settings: AppSettingsStuff {
            toggles,
            switches: app_settings_switches,
        },
        page_count,
    }
}

/// The fixed pages plus, optionally, the app's own Settings.bundle toggles.
enum Pages {
    Fixed([Vec<RowKind>; 3]),
    WithSettings([Vec<RowKind>; 3], Vec<AppToggle>),
}

impl std::ops::Index<usize> for Pages {
    type Output = [RowKind];
    fn index(&self, i: usize) -> &[RowKind] {
        match self {
            Pages::Fixed(pages) => &pages[i],
            Pages::WithSettings(pages, _) => &pages[i],
        }
    }
}

fn make_centered_label(
    env: &mut Environment,
    super_view: id,
    text: id,
    super_view_size: CGSize,
    center_y: CGFloat,
) -> id {
    let frame = CGRect {
        origin: CGPoint {
            x: 0.0,
            y: center_y - 30.0 / 2.0,
        },
        size: CGSize {
            width: super_view_size.width,
            height: 30.0,
        },
    };
    let label: id = msg_class![env; UILabel alloc];
    let label: id = msg![env; label initWithFrame:frame];
    () = msg![env; label setText:text];
    () = msg![env; label setTextAlignment:UITextAlignmentCenter];
    () = msg![env; super_view addSubview:label];
    label
}

/// The app's display name as an `NSString*`, for labelling its settings pages.
fn update_quick_options_page(env: &mut Environment, stuff: &QuickOptionsStuff, page_idx: usize) {
    for (i, page_view) in stuff.page_views.iter().enumerate() {
        () = msg![env; (*page_view) setHidden:(i != page_idx)];
    }
    let text = ns_string::from_rust_string(
        env,
        format!("{} / {}", page_idx + 1, stuff.page_views.len()),
    );
    () = msg![env; (stuff.page_label) setText:text];
}
