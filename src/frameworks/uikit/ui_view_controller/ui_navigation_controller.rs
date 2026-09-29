/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UINavigationController`.

use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::{ns_array, NSUInteger};
use crate::objc::{
    autorelease, id, impl_HostObject_with_superclass, msg, msg_class, nil, objc_classes, release,
    retain, ClassExports, NSZonePtr, SEL,
};
use crate::Environment;

// TODO: the optional toolbar along the bottom
// TODO: animations

#[derive(Default)]
struct UINavigationControllerHostObject {
    superclass: super::UIViewControllerHostObject,
    /// something implementing UINavigationControllerDelegate
    delegate: id,
    /// Navigation stack of view controllers, non-retaining
    /// (we explicitly retain/release on push/pop messages)
    navigation_stack: Vec<id>,
    /// The bar at the top of this controller's view. Retained while the
    /// controller is alive, even when hidden.
    /// `UINavigationBar*`
    navigation_bar: id,
    /// Whether the bar is hidden. A hidden bar has no superview, so it takes
    /// no room at the top of the controller's view.
    navigation_bar_hidden: bool,
}
impl_HostObject_with_superclass!(UINavigationControllerHostObject);

/// Height of the navigation bar, excluding the status bar.
const NAVIGATION_BAR_HEIGHT: CGFloat = 44.0;

/// Adds the controller's bar to its view if it should be visible.
///
/// The bar sits above the content view controller's view, so it is inserted
/// at the front of the subview list. The content view is not shrunk to make
/// room for it, matching what this emulator does for the status bar.
fn add_navigation_bar_to_view(env: &mut Environment, this: id) {
    if env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_bar_hidden {
        return;
    }
    let bar = env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_bar;
    if bar == nil {
        return;
    }
    let self_view: id = msg![env; this view];
    if self_view == nil {
        return;
    }
    let view_bounds: CGRect = msg![env; self_view bounds];
    () = msg![env; bar setFrame:(CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize { width: view_bounds.size.width, height: NAVIGATION_BAR_HEIGHT },
    })];
    () = msg![env; self_view addSubview:bar];
}

/// Pushes `view_controller`'s navigation item onto the controller's bar, if
/// there is a bar. This is what makes a pushed controller's `title` appear.
fn show_navigation_item_for(env: &mut Environment, this: id, view_controller: id) {
    let bar = env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_bar;
    if bar == nil {
        return;
    }
    let item: id = msg![env; view_controller navigationItem];
    if item != nil {
        () = msg![env; bar pushNavigationItem:item animated:false];
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UINavigationController: UIViewController

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UINavigationControllerHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithRootViewController:(id)root_vc { // UIViewController *
    () = msg![env; this pushViewController:root_vc animated:false];
    this
}

// weak/non-retaining
- (())setDelegate:(id)delegate { // something implementing UINavigationControllerDelegate
    log_dbg!("[(UINavigationController*){:?} setDelegate:{:?}]", this, delegate);
    let host_object = env.objc.borrow_mut::<UINavigationControllerHostObject>(this);
    host_object.delegate = delegate;
}
- (id)delegate {
    env.objc.borrow::<UINavigationControllerHostObject>(this).delegate
}

- (())pushViewController:(id)view_controller // UIViewController *
                animated:(bool)_animated {
    let stack = &mut env.objc.borrow_mut::<UINavigationControllerHostObject>(this).navigation_stack;
    assert!(!stack.contains(&view_controller));
    stack.push(view_controller);
    retain(env, view_controller);

    let delegate = env.objc.borrow::<UINavigationControllerHostObject>(this).delegate;
    let sel: SEL = env
        .objc
        .register_host_selector(
            "navigationController:willShowViewController:animated:".to_string(),
            &mut env.mem
        );
    let responds: bool = msg![env; delegate respondsToSelector:sel];
    if responds {
        () = msg![env; delegate navigationController:this willShowViewController:view_controller animated:false];
    }
    let self_view: id = msg![env; this view];
    let vc_view: id = msg![env; view_controller view];
    // TODO: animations
    () = msg![env; view_controller viewWillAppear:false];
    () = msg![env; self_view addSubview:vc_view];
    show_navigation_item_for(env, this, view_controller);
    () = msg![env; view_controller viewDidAppear:false];
    let sel: SEL = env
        .objc
        .register_host_selector(
            "navigationController:didShowViewController:animated:".to_string(),
            &mut env.mem
        );
    let responds: bool  = msg![env; delegate respondsToSelector:sel];
    if responds {
        () = msg![env; delegate navigationController:this didShowViewController:view_controller animated:false];
    }
}

- (id)topViewController {
    if let Some(top_vc) = env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_stack.last() {
        *top_vc
    } else {
        nil
    }
}

- (id)viewControllers {
    let vcs = env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_stack.to_vec();
    for vc in &vcs {
        retain(env, *vc);
    }
    let res = ns_array::from_vec(env, vcs);
    autorelease(env, res)
}
- (())setViewControllers:(id)controllers { // NSArray *
    msg![env; this setViewControllers:controllers animated:false]
}

- (())setViewControllers:(id)controllers // NSArray *
                animated:(bool)animated {
    assert!(!animated);

    // Clean existing view controllers
    let self_view = env.objc.borrow::<UINavigationControllerHostObject>(this).superclass.view;
    let mut stack = std::mem::take(&mut env.objc.borrow_mut::<UINavigationControllerHostObject>(this).navigation_stack);
    // TODO: shall we drain in reverse order? does it matter?
    for controller in stack.drain(..) {
        let vc_view = env.objc.borrow::<super::UIViewControllerHostObject>(controller).view;
        let vc_view_superview = msg![env; vc_view superview];
        assert_eq!(self_view, vc_view_superview);
        // TODO: view{Will,Did}Disappear: messages for vc?
        () = msg![env; vc_view removeFromSuperview];

        release(env, controller);
    }

    let mut tmp_stack: Vec<id> = Vec::new();
    let count: NSUInteger = msg![env; controllers count];
    // TODO: zero count
    assert!(count > 0);
    for i in 0..(count - 1) {
        let next: id = msg![env; controllers objectAtIndex:i];
        tmp_stack.push(next);
        retain(env, next);
    }
    env.objc.borrow_mut::<UINavigationControllerHostObject>(this).navigation_stack = tmp_stack;

    // The n-1 element in the controllers array is special and need to be pushed
    // TODO: double check this behavior
    let last_vc: id = msg![env; controllers objectAtIndex:(count - 1)];
    () = msg![env; this pushViewController:last_vc animated:animated];
}

// Creates the bar on first use and returns it. The bar belongs to the
// controller's view: it is added as a subview and takes the full width of the
// top of that view.
- (id)navigationBar {
    let existing = env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_bar;
    if existing != nil {
        return existing;
    }

    let bar: id = msg_class![env; UINavigationBar alloc];
    let bar: id = msg![env; bar init];
    retain(env, bar);
    env.objc.borrow_mut::<UINavigationControllerHostObject>(this).navigation_bar = bar;

    // The bar mirrors the top of the stack: a view controller's own
    // navigationItem (which is what its title and bar button items are set on)
    // is pushed whenever that controller is shown.
    if let Some(top_vc) = env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_stack.last().copied() {
        let item: id = msg![env; top_vc navigationItem];
        if item != nil {
            () = msg![env; bar pushNavigationItem:item animated:false];
        }
    }

    add_navigation_bar_to_view(env, this);
    bar
}

- (())setNavigationBarHidden:(bool)hidden {
    msg![env; this setNavigationBarHidden:hidden animated:false]
}

- (())setNavigationBarHidden:(bool)hidden animated:(bool)_animated {
    env.objc.borrow_mut::<UINavigationControllerHostObject>(this).navigation_bar_hidden = hidden;
    let bar = env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_bar;
    if bar == nil {
        return;
    }
    if hidden {
        () = msg![env; bar removeFromSuperview];
    } else {
        add_navigation_bar_to_view(env, this);
    }
}

- (bool)isNavigationBarHidden {
    env.objc.borrow::<UINavigationControllerHostObject>(this).navigation_bar_hidden
}

@end

};
