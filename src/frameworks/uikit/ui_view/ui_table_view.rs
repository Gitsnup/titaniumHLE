/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `UITableView` and `UITableViewCell`.
//!
//! This is a functional but simplified implementation:
//!
//! * `reloadData` lays out cells as subviews of the table view, one per row,
//!   and computes the content size from the row height. Cells are laid out in
//!   a single column, which matches `UITableViewStylePlain` behavior.
//! * Scrolling is inherited from `UIScrollView`.
//! * Cell reuse is supported via a reuse queue keyed on the reuse identifier.

use std::collections::HashMap;

use crate::frameworks::core_graphics::{CGFloat, CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::ns_string::to_rust_string;
use crate::frameworks::foundation::{NSInteger, NSUInteger};
use crate::objc::{
    id, impl_HostObject_with_superclass, msg, msg_class, msg_super, nil, objc_classes, retain,
    todo_objc_setter, ClassExports, NSZonePtr,
};

pub type UITableViewStyle = NSInteger;
pub const UITableViewStylePlain: UITableViewStyle = 0;
pub const UITableViewStyleGrouped: UITableViewStyle = 1;

#[derive(Default)]
pub struct UITableViewCellHostObject {
    superclass: super::UIViewHostObject,
    /// `NSString*`, may be `nil`
    reuse_identifier: id,
    selected: bool,
    selection_style: UITableViewCellSelectionStyle,
}
impl_HostObject_with_superclass!(UITableViewCellHostObject);

pub type UITableViewCellSelectionStyle = NSInteger;

pub struct UITableViewHostObject {
    superclass: super::ui_scroll_view::UIScrollViewHostObject,
    style: UITableViewStyle,
    /// `UITableViewDataSource`, weak reference
    data_source: id,
    row_height: CGFloat,
    /// Cells currently displayed, in display order.
    cells: Vec<id>,
    /// Reuse pool, keyed by reuse identifier.
    reuse_queue: HashMap<String, Vec<id>>,
}
impl_HostObject_with_superclass!(UITableViewHostObject);
impl Default for UITableViewHostObject {
    fn default() -> Self {
        UITableViewHostObject {
            superclass: Default::default(),
            style: UITableViewStylePlain,
            data_source: nil,
            row_height: 44.0,
            cells: Vec::new(),
            reuse_queue: HashMap::new(),
        }
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation UITableView: UIScrollView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UITableViewHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithFrame:(CGRect)frame {
    msg_super![env; this initWithFrame:frame]
}

- (id)initWithFrame:(CGRect)frame
              style:(UITableViewStyle)style {
    let this: id = msg_super![env; this initWithFrame:frame];
    env.objc.borrow_mut::<UITableViewHostObject>(this).style = style;
    this
}

- (UITableViewStyle)style {
    env.objc.borrow::<UITableViewHostObject>(this).style
}

- (id)dataSource {
    env.objc.borrow::<UITableViewHostObject>(this).data_source
}
- (())setDataSource:(id)data_source {
    env.objc.borrow_mut::<UITableViewHostObject>(this).data_source = data_source;
}

- (CGFloat)rowHeight {
    env.objc.borrow::<UITableViewHostObject>(this).row_height
}
- (())setRowHeight:(CGFloat)row_height {
    env.objc.borrow_mut::<UITableViewHostObject>(this).row_height = row_height;
}

- (())reloadData {
    let data_source: id = env.objc.borrow::<UITableViewHostObject>(this).data_source;
    if data_source == nil {
        return;
    }

    // Detach and pool the previously displayed cells.
    let old_cells = std::mem::take(
        &mut env.objc.borrow_mut::<UITableViewHostObject>(this).cells,
    );
    for cell in old_cells {
        let reuse_identifier: id = env
            .objc
            .borrow::<UITableViewCellHostObject>(cell)
            .reuse_identifier;
        if reuse_identifier != nil {
            retain(env, reuse_identifier);
            let key = to_rust_string(env, reuse_identifier).into_owned();
            retain(env, cell);
            env.objc
                .borrow_mut::<UITableViewHostObject>(this)
                .reuse_queue
                .entry(key)
                .or_default()
                .push(cell);
        }
        () = msg![env; cell removeFromSuperview];
    }

    let bounds: CGRect = msg![env; this bounds];
    let row_height: CGFloat = env.objc.borrow::<UITableViewHostObject>(this).row_height;

    let sections_sel = env.objc.register_host_selector(
        "numberOfSectionsInTableView:".to_string(),
        &mut env.mem,
    );
    let responds: bool = msg![env; data_source respondsToSelector:sections_sel];
    let sections: NSInteger = if responds {
        msg![env; data_source numberOfSectionsInTableView:this]
    } else {
        // If not implemented, the data source must implement
        // tableView:numberOfRowsInSection:, which is treated as one section.
        1
    };

    let rows_sel = env.objc.register_host_selector(
        "tableView:numberOfRowsInSection:".to_string(),
        &mut env.mem,
    );
    let cell_sel = env.objc.register_host_selector(
        "tableView:cellForRowAtIndexPath:".to_string(),
        &mut env.mem,
    );

    let mut content_height: CGFloat = 0.0;
    for section in 0..sections {
        let rows: NSInteger =
            crate::objc::msg_send(env, (data_source, rows_sel, this, section));
        for row in 0..rows {
            let index_path: id = msg_class![env; this indexPathForRow:row inSection:section];
            let cell: id = crate::objc::msg_send(env, (data_source, cell_sel, this, index_path));
            if cell == nil {
                continue;
            }
            let frame = CGRect {
                origin: CGPoint { x: 0.0, y: content_height },
                size: CGSize { width: bounds.size.width, height: row_height },
            };
            () = msg![env; cell setFrame:frame];
            () = msg![env; this addSubview:cell];
            env.objc
                .borrow_mut::<UITableViewHostObject>(this)
                .cells
                .push(cell);
            content_height += row_height;
        }
    }

    let content_size = CGSize { width: bounds.size.width, height: content_height };
    () = msg![env; this setContentSize:content_size];
}

- (id)dequeueReusableCellWithIdentifier:(id)identifier { // NSString*
    let key = to_rust_string(env, identifier).into_owned();
    let cell = env
        .objc
        .borrow_mut::<UITableViewHostObject>(this)
        .reuse_queue
        .entry(key)
        .or_default()
        .pop();
    match cell {
        Some(cell) => cell,
        None => nil,
    }
}

- (id)indexPathForRow:(NSUInteger)row
            inSection:(NSUInteger)section {
    msg_class![env; this indexPathForRow:row inSection:section]
}

- (())scrollToRowAtIndexPath:(id)index_path
           atScrollPosition:(NSInteger)_scroll_position
                   animated:(bool)_animated {
    let row: NSUInteger = msg![env; index_path row];
    let row_height: CGFloat = env.objc.borrow::<UITableViewHostObject>(this).row_height;
    let bounds: CGRect = msg![env; this bounds];
    let content_size: CGSize = msg![env; this contentSize];

    let target_y = row as CGFloat * row_height;
    let view_height = bounds.size.height;
    let max_offset = (content_size.height - view_height).max(0.0);
    let new_y = (target_y - (view_height / 2.0) + (row_height / 2.0)).max(0.0).min(max_offset);
    let offset = CGPoint { x: 0.0, y: new_y };
    () = msg![env; this setContentOffset:offset];
}

@end

@implementation UITableViewCell: UIView

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::<UITableViewCellHostObject>::default();
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithFrame:(CGRect)frame {
    msg_super![env; this initWithFrame:frame]
}

- (id)initWithFrame:(CGRect)frame
    reuseIdentifier:(id)reuse_identifier { // NSString*
    let this: id = msg_super![env; this initWithFrame:frame];
    env.objc.borrow_mut::<UITableViewCellHostObject>(this).reuse_identifier = reuse_identifier;
    if reuse_identifier != nil {
        retain(env, reuse_identifier);
    }
    this
}

- (id)initWithStyle:(UITableViewStyle)_style
    reuseIdentifier:(id)reuse_identifier { // NSString*
    // iOS 3.0+ designated initializer, forwards to the 2.0-style initializer.
    msg![env; this initWithFrame:(<CGRect as Default>::default()) reuseIdentifier:reuse_identifier]
}

- (id)reuseIdentifier { // NSString*
    env.objc.borrow::<UITableViewCellHostObject>(this).reuse_identifier
}

- (())prepareForReuse {
}

- (bool)isSelected {
    env.objc.borrow::<UITableViewCellHostObject>(this).selected
}
- (())setSelected:(bool)selected {
    env.objc.borrow_mut::<UITableViewCellHostObject>(this).selected = selected;
}
- (())setSelected:(bool)selected
         animated:(bool)_animated {
    env.objc.borrow_mut::<UITableViewCellHostObject>(this).selected = selected;
}

- (UITableViewCellSelectionStyle)selectionStyle {
    env.objc.borrow::<UITableViewCellHostObject>(this).selection_style
}
- (())setSelectionStyle:(UITableViewCellSelectionStyle)selection_style {
    env.objc.borrow_mut::<UITableViewCellHostObject>(this).selection_style = selection_style;
}

- (())setHighlighted:(bool)highlighted {
    todo_objc_setter!(this, highlighted);
}
- (())setHighlighted:(bool)highlighted
            animated:(bool)_animated {
    todo_objc_setter!(this, highlighted);
}

@end

};
